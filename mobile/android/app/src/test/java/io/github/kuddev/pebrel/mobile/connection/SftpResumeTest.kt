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
import java.security.MessageDigest

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
        val data = ByteArray(SftpClient.BATCH * 3 + 37) { (it % 239).toByte() }
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
        assertEquals(listOf(0L, SftpClient.BATCH.toLong()), starts)
        assertEquals(1, tokens.size)
        assertEquals(2, recovered)
        assertEquals("a timed-out batch shrinks so slow links can confirm progress", SftpClient.BATCH / 2, writes[2])
        assertEquals("fast recovery restores the full window", SftpClient.BATCH, writes[4])
        assertArrayEquals(data, remote)
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
