package io.github.kuddev.pebrel.mobile.connection

import android.app.Application
import android.util.Base64
import io.github.kuddev.pebrel.ssh.NativeSshException
import kotlinx.coroutines.*
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.ConscryptMode
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.io.IOException
import java.security.MessageDigest
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], application = Application::class)
@ConscryptMode(ConscryptMode.Mode.OFF)
class SftpResumeTest {
    private fun file(size: Int, revision: String = "v1") = JSONObject().put("path", "/source").put("name", "source")
        .put("kind", "file").put("size", size).put("revision", revision)

    @Test fun downloadRetainsConfirmedBytesAndReopensAfterConnectionReplacement() = runBlocking {
        val data = ByteArray(SftpClient.CHUNK * 3 + 7) { (it % 251).toByte() }
        val starts = mutableListOf<Long>()
        var disconnect = true
        var recovered = 0
        val client = SftpClient({ request ->
            when (request.getString("op")) {
                "open_read" -> {
                    starts += request.getLong("offset")
                    if (starts.size > 1) assertEquals("v1", request.getString("revision"))
                    JSONObject().put("transfer", starts.size).put("file", file(data.size))
                }
                "read" -> {
                    val offset = request.getInt("offset")
                    if (offset > 0 && disconnect) { disconnect = false; throw NativeSshException("SFTP_CLOSED") }
                    val end = (offset + SftpClient.CHUNK).coerceAtMost(data.size)
                    JSONObject().put("offset", offset).put("next_offset", end).put("eof", end == data.size)
                        .put("data", Base64.encodeToString(data.copyOfRange(offset, end), Base64.NO_WRAP))
                }
                "close" -> JSONObject()
                else -> error("unexpected operation")
            }
        }, recover = { recovered++ })
        val sink = ByteArrayOutputStream()
        val progress = mutableListOf<Long>()
        val waiting = mutableListOf<Boolean>()
        client.download("/source", sink, waiting = { waiting += it }) { count, _ -> progress += count }
        assertArrayEquals(data, sink.toByteArray())
        assertEquals(listOf(0L, SftpClient.CHUNK.toLong()), starts)
        assertEquals(1, recovered)
        assertTrue(waiting.contains(true))
        assertTrue(progress.zipWithNext().all { (a, b) -> b > a })
    }

    @Test fun changedDownloadFailsWithoutAppendingAnotherRevision() = runBlocking {
        var opens = 0
        var recoveries = 0
        val client = SftpClient({ request ->
            when (request.getString("op")) {
                "open_read" -> JSONObject().put("transfer", ++opens).put("file", file(20, if (opens == 1) "v1" else "v2"))
                "read" -> throw NativeSshException("NETWORK")
                "close" -> JSONObject()
                else -> error("unexpected operation")
            }
        }, recover = { recoveries++ })
        val sink = ByteArrayOutputStream()
        val failure = runCatching { client.download("/source", sink) }.exceptionOrNull() as NativeSshException
        assertEquals("SFTP_CHANGED", failure.code)
        assertEquals(1, recoveries)
        assertEquals(0, sink.size())
    }

    @Test fun uploadReplaysOnlyTheUnconfirmedBatchAndReconcilesALostCommitReply() = runBlocking {
        val data = ByteArray(SftpClient.UPLOAD_BATCH * 3 + 37) { (it % 239).toByte() }
        var remote = ByteArray(0)
        val starts = mutableListOf<Long>()
        val tokens = mutableSetOf<String>()
        var lostWrite = false
        var committed = false
        var recovered = 0
        val writes = mutableListOf<Int>()
        val client = SftpClient({ request ->
            when (request.getString("op")) {
                "begin_upload" -> {
                    val offset = request.getInt("offset")
                    starts += offset.toLong(); tokens += request.getString("token")
                    assertEquals(starts.size > 1, request.getBoolean("resume"))
                    remote = remote.copyOf(offset)
                    JSONObject().put("transfer", starts.size)
                }
                "write_batch" -> {
                    val offset = request.getInt("offset")
                    val bytes = Base64.decode(request.getString("data"), Base64.NO_WRAP)
                    writes += bytes.size
                    remote = remote.copyOf(offset + bytes.size)
                    bytes.copyInto(remote, offset)
                    if (offset > 0 && !lostWrite) { lostWrite = true; throw NativeSshException("TIMEOUT") }
                    JSONObject().put("next_offset", offset + bytes.size)
                }
                "commit" -> { committed = true; throw NativeSshException("SFTP_CLOSED") }
                "resolve_upload" -> {
                    assertTrue(committed)
                    assertEquals(remote.size.toLong(), request.getLong("size"))
                    assertEquals(MessageDigest.getInstance("SHA-256").digest(remote).joinToString("") { "%02x".format(it.toInt() and 255) }, request.getString("digest"))
                    JSONObject().put("committed", true).put("path", "/target")
                }
                "close", "abort_upload" -> JSONObject()
                else -> error("unexpected operation")
            }
        }, recover = { recovered++ })
        assertEquals("/target", client.upload("/target", ByteArrayInputStream(data), data.size.toLong()))
        assertEquals(listOf(0L, SftpClient.UPLOAD_BATCH.toLong()), starts)
        assertEquals(1, tokens.size)
        assertEquals(2, recovered)
        assertEquals("a timed-out batch shrinks so slow links can confirm progress", SftpClient.UPLOAD_BATCH / 2, writes[2])
        assertEquals("fast recovery restores the full window", SftpClient.UPLOAD_BATCH, writes[4])
        assertArrayEquals(data, remote)
    }

    @Test fun uploadPreparesOneBatchAheadWhileTheNetworkWaitsForAcknowledgement() = runBlocking {
        val data = ByteArray(SftpClient.UPLOAD_BATCH * 2 + 37) { (it % 251).toByte() }
        val reads = AtomicInteger()
        val nextRead = CountDownLatch(1)
        val source = object : ByteArrayInputStream(data) {
            override fun read(target: ByteArray, offset: Int, length: Int): Int {
                val count = super.read(target, offset, length)
                if (reads.incrementAndGet() == 2) nextRead.countDown()
                return count
            }
        }
        val remote = ByteArrayOutputStream()
        val progress = mutableListOf<Long>()
        val client = SftpClient({ request ->
            when (request.getString("op")) {
                "begin_upload" -> JSONObject().put("transfer", 1)
                "write_batch" -> {
                    if (request.getInt("offset") == 0) {
                        assertTrue("source preparation must overlap a blocked upload", nextRead.await(5, TimeUnit.SECONDS))
                        assertEquals("prefetch must retain at most one future batch", 2, reads.get())
                    }
                    assertEquals(remote.size(), request.getInt("offset"))
                    remote.write(Base64.decode(request.getString("data"), Base64.NO_WRAP))
                    JSONObject().put("next_offset", remote.size())
                }
                "commit" -> JSONObject().put("path", "/target")
                "close", "abort_upload" -> JSONObject()
                else -> error("unexpected operation")
            }
        })
        client.upload("/target", source, data.size.toLong(), progress = { count, _ -> progress += count })
        assertArrayEquals(data, remote.toByteArray())
        assertEquals(listOf(SftpClient.UPLOAD_BATCH.toLong(), SftpClient.UPLOAD_BATCH * 2L, data.size.toLong()), progress)
    }

    @Test fun uploadCoalescesShortReadsAndCommitsEmptyFiles() = runBlocking {
        for (size in listOf(0, SftpClient.UPLOAD_BATCH + 37)) {
            val data = ByteArray(size) { (it % 239).toByte() }
            val source = object : ByteArrayInputStream(data) {
                var first = true
                override fun read(target: ByteArray, offset: Int, length: Int): Int {
                    if (first) { first = false; return 0 }
                    return super.read(target, offset, minOf(length, 701))
                }
            }
            val remote = ByteArrayOutputStream()
            val sizes = mutableListOf<Int>()
            val client = SftpClient({ request ->
                when (request.getString("op")) {
                    "begin_upload" -> JSONObject().put("transfer", 1)
                    "write_batch" -> {
                        val bytes = Base64.decode(request.getString("data"), Base64.NO_WRAP)
                        sizes += bytes.size
                        assertEquals(remote.size(), request.getInt("offset"))
                        remote.write(bytes)
                        JSONObject().put("next_offset", remote.size())
                    }
                    "commit" -> JSONObject().put("path", "/target")
                    "close", "abort_upload" -> JSONObject()
                    else -> error("unexpected operation")
                }
            })
            assertEquals("/target", client.upload("/target", source, null))
            assertEquals(if (size == 0) emptyList<Int>() else listOf(SftpClient.UPLOAD_BATCH, 37), sizes)
            assertArrayEquals(data, remote.toByteArray())
        }
    }

    @Test fun cancellationInterruptsThePrefetchReaderBeforeCleaningTheStagingUpload() = runBlocking {
        val reading = CountDownLatch(1)
        val stopped = CountDownLatch(1)
        val aborted = AtomicBoolean()
        val source = object : InputStream() {
            var first = true
            override fun read(): Int = error("bulk reads only")
            override fun read(target: ByteArray, offset: Int, length: Int): Int {
                if (first) { first = false; target.fill(1, offset, offset + length); return length }
                reading.countDown()
                try { CountDownLatch(1).await(); return -1 } finally { stopped.countDown() }
            }
        }
        val client = SftpClient({ request ->
            when (request.getString("op")) {
                "begin_upload" -> JSONObject().put("transfer", 1)
                "write_batch" -> JSONObject().put("next_offset", SftpClient.UPLOAD_BATCH)
                "close" -> JSONObject()
                "abort_upload" -> { aborted.set(true); JSONObject() }
                else -> error("cancelled upload must not be published")
            }
        })
        val job = launch { client.upload("/target", source, null) }
        withContext(Dispatchers.IO) { assertTrue(reading.await(5, TimeUnit.SECONDS)) }
        withTimeout(5000) { job.cancelAndJoin() }
        assertEquals(0L, stopped.count)
        assertTrue(aborted.get())
    }

    @Test fun sourceReadFailureAbortsTheStagingUpload() = runBlocking {
        var aborted = false
        val source = object : InputStream() {
            override fun read(): Int = throw IOException("source read failed")
        }
        val client = SftpClient({ request ->
            when (request.getString("op")) {
                "begin_upload" -> JSONObject().put("transfer", 1)
                "close" -> JSONObject()
                "abort_upload" -> { aborted = true; JSONObject() }
                else -> error("a source failure must not publish an upload")
            }
        })
        assertEquals("source read failed", runCatching { client.upload("/target", source, null) }.exceptionOrNull()?.message)
        assertTrue(aborted)
    }

    @Test fun cancellationWhileWaitingStopsRetriesAndCleansOnlyItsStagingUpload() = runBlocking {
        val recovering = CompletableDeferred<Unit>()
        var aborted = false
        val client = SftpClient({ request ->
            when (request.getString("op")) {
                "begin_upload" -> throw NativeSshException("NETWORK")
                "abort_upload" -> { aborted = true; JSONObject() }
                else -> error("must not publish or remove the destination")
            }
        }, recover = { recovering.complete(Unit); awaitCancellation() })
        val job = launch { client.upload("/target", ByteArrayInputStream(byteArrayOf(1)), 1) }
        withTimeout(5000) { recovering.await() }
        job.cancelAndJoin()
        assertTrue(aborted)
    }

    @Test fun permissionErrorsNeverEnterTheReconnectLoop() = runBlocking {
        val client = SftpClient({ throw NativeSshException("SFTP_PERMISSION") }, recover = { fail("must not retry permission failures") })
        assertEquals("SFTP_PERMISSION", (runCatching { client.download("/source", ByteArrayOutputStream()) }.exceptionOrNull() as NativeSshException).code)
    }
}
