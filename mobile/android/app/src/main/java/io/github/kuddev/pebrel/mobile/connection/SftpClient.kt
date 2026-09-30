package io.github.kuddev.pebrel.mobile.connection

import android.util.Base64
import io.github.kuddev.pebrel.ssh.NativeSshException
import kotlinx.coroutines.*
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.io.OutputStream

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
class SftpClient(private val execute: (JSONObject) -> JSONObject, private val active: () -> Boolean = { true }) {
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
        val file = download(path, bytes, MAX_PREVIEW_BYTES, progress)
        return SftpContent(file, bytes.toByteArray())
    }

    suspend fun download(path: String, sink: OutputStream, maximum: Long = Long.MAX_VALUE,
                         progress: (Long, Long) -> Unit = { _, _ -> }): SftpEntry = withContext(Dispatchers.IO) {
        ensureActive()
        val opened = withContext(NonCancellable) { request("open_read", JSONObject().put("path", path)) }
        val transfer = opened.getLong("transfer")
        try {
            ensureActive()
            val file = SftpEntry.parse(opened.getJSONObject("file"))
            val size = file.size ?: throw NativeSshException("SFTP_FILE_TYPE")
            if (size > maximum) throw NativeSshException("SFTP_TOO_LARGE")
            var offset = 0L
            while (true) {
                ensureActive()
                val chunk = request("read", JSONObject().put("transfer", transfer).put("offset", offset))
                val bytes = Base64.decode(chunk.getString("data"), Base64.NO_WRAP)
                check(bytes.size <= CHUNK && chunk.getLong("offset") == offset)
                check(offset + bytes.size <= size && chunk.getLong("next_offset") == offset + bytes.size)
                sink.write(bytes)
                offset += bytes.size
                progress(offset, size)
                if (chunk.getBoolean("eof")) { check(offset == size); break }
                check(bytes.isNotEmpty())
            }
            sink.flush()
            file
        } finally { release(transfer) }
    }

    suspend fun upload(path: String, source: InputStream, size: Long?,
                       progress: (Long, Long?) -> Unit = { _, _ -> }): String = withContext(Dispatchers.IO) {
        ensureActive()
        val transfer = withContext(NonCancellable) { request("begin_upload", JSONObject().put("path", path)) }.getLong("transfer")
        try {
            ensureActive()
            var offset = 0L
            val buffer = ByteArray(CHUNK)
            while (true) {
                ensureActive()
                val count = source.read(buffer)
                if (count < 0) break
                if (count == 0) continue
                val data = Base64.encodeToString(buffer, 0, count, Base64.NO_WRAP)
                val written = request("write", JSONObject().put("transfer", transfer).put("offset", offset).put("data", data))
                check(written.getLong("next_offset") == offset + count)
                offset += count
                progress(offset, size)
            }
            if (size != null && size != offset) throw NativeSshException("SFTP_CHANGED")
            request("commit", JSONObject().put("transfer", transfer)).getString("path")
        } finally { release(transfer) }
    }

    private suspend fun release(transfer: Long) = withContext(NonCancellable + Dispatchers.IO) {
        // 取消只关闭自己的远程 handle / 临时上传，不关闭用户仍在使用的 SSH shell。
        if (active()) runCatching { request("close", JSONObject().put("transfer", transfer)) }
        Unit
    }

    companion object {
        const val CHUNK = 32 * 1024
        const val MAX_PREVIEW_BYTES = 16L * 1024 * 1024
    }
}

fun sftpChild(parent: String, name: String): String {
    require(name.isNotBlank() && name !in setOf(".", "..") && '/' !in name && name.none(Char::isISOControl))
    return "${parent.trimEnd('/')}/$name"
}

fun sftpParent(path: String): String = path.trimEnd('/').substringBeforeLast('/', "").ifBlank { "/" }
