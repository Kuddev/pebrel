package io.github.kuddev.pebrel.mobile.connection

import android.content.Context
import android.net.Uri
import android.util.Base64
import androidx.test.core.app.ApplicationProvider
import io.github.kuddev.pebrel.ssh.NativeSshException
import kotlinx.coroutines.*
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.ConscryptMode
import org.robolectric.annotation.Config
import java.io.ByteArrayOutputStream
import java.io.File

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], application = android.app.Application::class)
@ConscryptMode(ConscryptMode.Mode.OFF) // Transfer tests borrow SFTP and never perform TLS or authentication.
class TerminalAttachmentsTest {
    private val context get() = ApplicationProvider.getApplicationContext<Context>()

    private class Server {
        val directories = mutableSetOf("/home/user")
        val bytes = ByteArrayOutputStream()
        val operations = mutableListOf<String>()
        var target = ""
        var failWrite = false
        var afterWrite: () -> Unit = {}
        val client = SftpClient({ request ->
            val op = request.getString("op")
            operations += op
            val path = request.optString("path").let { if (it == ".") "/home/user" else it }
            when (op) {
                "stat" -> {
                    if (path !in directories) throw NativeSshException("SFTP_NOT_FOUND")
                    JSONObject().put("path", path).put("name", path.substringAfterLast('/'))
                        .put("kind", "directory").put("revision", "directory")
                }
                "mkdir" -> { assertTrue(directories.add(path)); JSONObject() }
                "begin_upload" -> { target = path; JSONObject().put("transfer", 7L) }
                "write_batch" -> {
                    if (failWrite) throw NativeSshException("SFTP_PERMISSION")
                    assertEquals(bytes.size().toLong(), request.getLong("offset"))
                    bytes.write(Base64.decode(request.getString("data"), Base64.NO_WRAP))
                    afterWrite()
                    JSONObject().put("next_offset", bytes.size().toLong())
                }
                "commit" -> JSONObject().put("path", target)
                "close" -> { assertEquals(7L, request.getLong("transfer")); JSONObject() }
                "remove" -> { assertTrue(directories.remove(path)); JSONObject() }
                else -> error("Unexpected operation: $op")
            }
        })
    }

    private suspend fun withSource(block: suspend (Uri, ByteArray) -> Unit) {
        val file = File.createTempFile("attachment", ".png", context.cacheDir)
        val bytes = ByteArray(70_000) { (it % 255).toByte() }
        file.writeBytes(bytes)
        try { block(Uri.fromFile(file), bytes) } finally { file.delete() }
        assertTrue(File(context.cacheDir, "pebrel-attachments").listFiles().orEmpty().isEmpty())
    }

    @Test fun uploadUsesOnlyTheAuthenticatedSftpChannelAndCommitsAllBytes() = runBlocking {
        val server = Server()
        withSource { uri, expected ->
            val path = TerminalAttachments.uploadSsh(context, uri, server.client)
            assertEquals(server.target, path)
            assertTrue(path.startsWith("/home/user/.cache/pebrel/attachments/"))
            assertArrayEquals(expected, server.bytes.toByteArray())
            assertEquals(listOf("commit", "close"), server.operations.takeLast(2))
        }
    }

    @Test fun failedUploadClosesItsTransferAndRemovesItsEmptyDirectory() = runBlocking {
        val server = Server().apply { failWrite = true }
        withSource { uri, _ ->
            try {
                TerminalAttachments.uploadSsh(context, uri, server.client)
                fail("expected transfer failure")
            } catch (error: AttachmentException) { assertEquals(AttachmentError.TRANSFER, error.code) }
            assertFalse(server.operations.contains("commit"))
            assertTrue(server.operations.contains("close"))
            assertFalse(server.directories.contains(server.target.substringBeforeLast('/')))
            assertTrue(server.client.stat(".").directory)
        }
    }

    @Test fun cancelledUploadReleasesOnlyItsOwnTransferAndLeavesSftpUsable() = runBlocking {
        val server = Server()
        withSource { uri, _ ->
            val upload = launch(start = CoroutineStart.LAZY) { TerminalAttachments.uploadSsh(context, uri, server.client) }
            server.afterWrite = { upload.cancel() }
            upload.start()
            upload.join()
            assertTrue(upload.isCancelled)
            assertFalse(server.operations.contains("commit"))
            assertTrue(server.operations.contains("close"))
            assertFalse(server.directories.contains(server.target.substringBeforeLast('/')))
            assertTrue(server.client.stat(".").directory)
        }
    }
}
