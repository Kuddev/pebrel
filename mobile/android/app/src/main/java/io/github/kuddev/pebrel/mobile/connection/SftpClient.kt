package io.github.kuddev.pebrel.mobile.connection

import android.util.Base64
import io.github.kuddev.pebrel.ssh.NativeSshException
import kotlinx.coroutines.*
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.io.OutputStream
import java.security.MessageDigest
import java.util.UUID

data class SftpEntry(val path: String, val name: String, val kind: String, val size: Long?,
                     val modified: Long?, val permissions: Int?, val revision: String) {
    val directory: Boolean get() = kind == "directory"
    companion object {
        fun parse(value: JSONObject) = SftpEntry(value.getString("path"), value.getString("name"), value.getString("kind"),
            if (value.isNull("size")) null else value.getLong("size"),
            if (value.isNull("modified")) null else value.getLong("modified"),
            if (value.isNull("permissions")) null else value.getInt("permissions"), value.getString("revision"))
    }
}
data class SftpListing(val path: String, val entries: List<SftpEntry>, val cursor: Long?, val skipped: Int)
data class SftpContent(val file: SftpEntry, val bytes: ByteArray)
data class SftpTab(val id: String, val session: String, val file: SftpEntry)

/** View operations borrow the already authenticated SSH connection; no extra password or socket. */
class SftpClient(private val execute: (JSONObject) -> JSONObject,
                 private val recover: (suspend () -> Unit)? = null,
                 private val active: () -> Boolean = { true }) {
    private suspend fun request(op: String, fields: JSONObject = JSONObject()): JSONObject = withContext(Dispatchers.IO) {
        ensureActive()
        if (!active()) throw NativeSshException("SFTP_CLOSED")
        val response = execute(fields.put("op", op))
        ensureActive()
        if (!active()) throw NativeSshException("SFTP_CLOSED")
        response
    }

    suspend fun list(path: String, cursor: Long? = null): SftpListing {
        currentCoroutineContext().ensureActive()
        // JNI 返回前的取消也必须取回新游标并归还，避免连续导航耗尽原生句柄。
        val result = withContext(NonCancellable) {
            request("list", JSONObject().put("path", path).apply { cursor?.let { put("cursor", it) } })
        }
        val next = if (result.isNull("cursor")) null else result.getLong("cursor")
        if (!currentCoroutineContext().isActive) {
            if (next != null) withContext(NonCancellable) { runCatching { closeList(next) } }
            currentCoroutineContext().ensureActive()
        }
        val rows = result.getJSONArray("entries")
        check(rows.length() <= 256)
        return SftpListing(result.getString("path"), (0 until rows.length()).map { SftpEntry.parse(rows.getJSONObject(it)) },
            next, result.optInt("skipped"))
    }

    suspend fun closeList(cursor: Long) { request("close_list", JSONObject().put("cursor", cursor)) }
    suspend fun stat(path: String): SftpEntry = SftpEntry.parse(request("stat", JSONObject().put("path", path)))
    suspend fun mkdir(path: String) { request("mkdir", JSONObject().put("path", path)) }
    suspend fun rename(entry: SftpEntry, destination: String) {
        request("rename", JSONObject().put("path", entry.path).put("destination", destination).put("revision", entry.revision))
    }
    suspend fun remove(entry: SftpEntry) {
        request("remove", JSONObject().put("path", entry.path).put("revision", entry.revision))
    }

    suspend fun preview(path: String, progress: (Long, Long) -> Unit = { _, _ -> }): SftpContent {
        val bytes = ByteArrayOutputStream()
        val file = download(path, bytes, MAX_PREVIEW_BYTES, progress = progress)
        return SftpContent(file, bytes.toByteArray())
    }

    suspend fun download(path: String, sink: OutputStream, maximum: Long = Long.MAX_VALUE,
                         waiting: (Boolean) -> Unit = {},
                         progress: (Long, Long) -> Unit = { _, _ -> }): SftpEntry = withContext(Dispatchers.IO) {
        var transfer: Long? = null
        var file: SftpEntry? = null
        var offset = 0L
        var batch = BATCH
        try {
            do {
                ensureActive()
                try {
                    if (transfer == null) {
                        val opened = withContext(NonCancellable) { request("open_read", JSONObject()
                            .put("path", file?.path ?: path).put("offset", offset).apply { file?.let { put("revision", it.revision) } }) }
                        transfer = opened.getLong("transfer")
                        val current = SftpEntry.parse(opened.getJSONObject("file"))
                        if (file != null && (file.revision != current.revision || file.path != current.path)) throw NativeSshException("SFTP_CHANGED")
                        file = current
                    }
                    val size = file?.size ?: throw NativeSshException("SFTP_FILE_TYPE")
                    if (size > maximum) throw NativeSshException("SFTP_TOO_LARGE")
                    val started = System.nanoTime()
                    val chunk = request("read", JSONObject().put("transfer", transfer).put("offset", offset).put("length", batch))
                    val bytes = Base64.decode(chunk.getString("data"), Base64.NO_WRAP)
                    check(bytes.size <= BATCH && chunk.getLong("offset") == offset)
                    check(offset + bytes.size <= size && chunk.getLong("next_offset") == offset + bytes.size)
                    sink.write(bytes)
                    offset += bytes.size
                    waiting(false)
                    progress(offset, size)
                    if (bytes.size == batch && System.nanoTime() - started < 5_000_000_000L) batch = (batch * 2).coerceAtMost(BATCH)
                    if (chunk.getBoolean("eof")) { check(offset == size); break }
                    check(bytes.isNotEmpty())
                } catch (error: NativeSshException) {
                    if (error.code in setOf("TIMEOUT", "SFTP_TIMEOUT")) batch = (batch / 2).coerceAtLeast(CHUNK)
                    awaitRecovery(error, waiting)
                    transfer = null // Native handles belong to the old subsystem, never the replacement.
                }
            } while (true)
            sink.flush()
            checkNotNull(file)
        } finally { transfer?.let { release(it) }; waiting(false) }
    }

    suspend fun upload(path: String, source: InputStream, size: Long?, waiting: (Boolean) -> Unit = {},
                       progress: (Long, Long?) -> Unit = { _, _ -> }): String = withContext(Dispatchers.IO) {
        val token = UUID.randomUUID().toString().replace("-", "")
        var transfer: Long? = null
        var offset = 0L
        var attempted = false
        var completed = false
        var batch = BATCH
        val digest = MessageDigest.getInstance("SHA-256")
        suspend fun open() {
            while (transfer == null) {
                ensureActive()
                val resume = attempted
                attempted = true // Retain identity even if the server's OPEN acknowledgement is lost.
                try {
                    transfer = withContext(NonCancellable) { request("begin_upload", JSONObject()
                        .put("path", path).put("token", token).put("offset", offset).put("resume", resume)) }.getLong("transfer")
                } catch (error: NativeSshException) { awaitRecovery(error, waiting) }
            }
        }
        try {
            open()
            val buffer = ByteArray(BATCH)
            while (true) {
                ensureActive()
                // A short local read is valid, including document providers with unknown sizes.
                val count = source.read(buffer)
                if (count < 0) break
                if (count == 0) { yield(); continue }
                var position = 0
                while (position < count) {
                    val length = minOf(batch, count - position)
                    try {
                        open()
                        val started = System.nanoTime()
                        val data = Base64.encodeToString(buffer, position, length, Base64.NO_WRAP)
                        val written = request("write_batch", JSONObject().put("transfer", transfer).put("offset", offset).put("data", data))
                        check(written.getLong("next_offset") == offset + length)
                        digest.update(buffer, position, length)
                        position += length
                        offset += length
                        waiting(false)
                        progress(offset, size)
                        if (length == batch && System.nanoTime() - started < 5_000_000_000L) batch = (batch * 2).coerceAtMost(BATCH)
                    } catch (error: NativeSshException) {
                        if (error.code in setOf("TIMEOUT", "SFTP_TIMEOUT")) batch = (batch / 2).coerceAtLeast(CHUNK)
                        awaitRecovery(error, waiting)
                        transfer = null
                    }
                }
                // Keep source bytes until acknowledged, including after shrinking a slow link's window.
            }
            if (size != null && size != offset) throw NativeSshException("SFTP_CHANGED")
            val hash = digest.digest().joinToString("") { "%02x".format(it.toInt() and 255) }
            var reconcile = false
            while (true) {
                ensureActive()
                try {
                    if (reconcile) {
                        val state = request("resolve_upload", JSONObject().put("path", path).put("token", token).put("size", offset).put("digest", hash))
                        if (state.getBoolean("committed")) return@withContext state.getString("path").also { completed = true }
                        open()
                    }
                    return@withContext request("commit", JSONObject().put("transfer", transfer)).getString("path").also { completed = true }
                } catch (error: NativeSshException) {
                    awaitRecovery(error, waiting)
                    transfer = null
                    reconcile = true
                }
            }
            @Suppress("UNREACHABLE_CODE") error("unreachable")
        } finally {
            transfer?.let { release(it) }
            // Never remove the final destination, even after an ambiguous RENAME acknowledgement.
            if (attempted && !completed && active()) withContext(NonCancellable) {
                runCatching { request("abort_upload", JSONObject().put("path", path).put("token", token)) }
            }
            waiting(false)
        }
    }

    private suspend fun awaitRecovery(error: NativeSshException, waiting: (Boolean) -> Unit) {
        val retry = recover ?: throw error
        if (error.code !in setOf("NETWORK", "TIMEOUT", "CLOSED", "SFTP_CLOSED", "SFTP_TIMEOUT", "SFTP_STALE")) throw error
        currentCoroutineContext().ensureActive()
        waiting(true)
        delay(500) // The SSH owner supplies reconnect backoff; avoid a busy loop for a reset subsystem.
        retry()
        currentCoroutineContext().ensureActive()
    }

    private suspend fun release(transfer: Long) = withContext(NonCancellable + Dispatchers.IO) {
        // 取消只关闭自己的远程 handle / 临时上传，不关闭用户仍在使用的 SSH shell。
        if (active()) runCatching { request("close", JSONObject().put("transfer", transfer)) }
        Unit
    }

    companion object {
        const val CHUNK = 32 * 1024
        const val BATCH = CHUNK * 64
        const val MAX_PREVIEW_BYTES = 16L * 1024 * 1024
    }
}

fun sftpChild(parent: String, name: String): String {
    require(name.isNotBlank() && name !in setOf(".", "..") && '/' !in name && name.none(Char::isISOControl))
    return "${parent.trimEnd('/')}/$name"
}

fun sftpParent(path: String): String = path.trimEnd('/').substringBeforeLast('/', "").ifBlank { "/" }
