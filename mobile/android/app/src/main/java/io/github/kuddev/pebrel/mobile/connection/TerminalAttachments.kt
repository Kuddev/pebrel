package io.github.kuddev.pebrel.mobile.connection

import android.content.ContentResolver
import android.content.Context
import android.database.Cursor
import android.net.Uri
import android.os.CancellationSignal
import android.provider.OpenableColumns
import io.github.kuddev.pebrel.mobile.session.LocalTerminalStorage
import io.github.kuddev.pebrel.ssh.NativeSshException
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.IOException
import java.io.InputStream
import java.nio.charset.StandardCharsets
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.util.Locale
import java.util.UUID
import java.util.concurrent.atomic.AtomicReference
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/** Stable failure categories for the attachment boundary. No remote output is exposed. */
enum class AttachmentError {
    TOO_LARGE,
    UNREADABLE,
    STORAGE,
    TRANSFER,
    INVALID_RESULT,
}

class AttachmentException(
    val code: AttachmentError,
    cause: Throwable? = null,
    val sshFailure: SshFailureKind? = null,
) : IOException("attachment_${code.name.lowercase(Locale.ROOT)}", cause)

/**
 * Imports a user-selected document without touching the interactive terminal.
 *
 * The local operation commits through a temporary file and the SSH operation stages the
 * document in the app cache. Both paths keep the source bounded and remove an incomplete
 * staging file when a coroutine is cancelled or the transfer fails.
 */
object TerminalAttachments {
    const val MAX_BYTES: Long = 16L * 1024L * 1024L

    private const val MAX_FILE_NAME_BYTES = 240
    private const val MAX_REMOTE_PATH_CHARS = 4096
    private const val STAGE_TIMEOUT_MS = 5 * 60_000L
    private const val TRANSFER_TIMEOUT_MS = 5 * 60_000L
    private const val COPY_BUFFER_BYTES = 16 * 1024

    /** Copies a SAF document into the shared terminal HOME and returns its canonical path. */
    suspend fun importLocal(context: Context, uri: Uri): String {
        val attachments = withContext(Dispatchers.IO) {
            try {
                val directory = File(LocalTerminalStorage.homePath(context), "attachments")
                ensureDirectory(directory)
                directory
            } catch (error: AttachmentException) {
                throw error
            } catch (error: Exception) {
                throw AttachmentException(AttachmentError.STORAGE, error)
            }
        }
        val staged = withTimeout(STAGE_TIMEOUT_MS) {
            stageSource(context.contentResolver, uri, attachments.resolve(".staging"))
        }
        var moved = false
        return try {
            withContext(Dispatchers.IO) {
                val target = uniqueTarget(attachments, staged.safeName)
                moveWithoutOverwrite(staged.file, target)
                moved = true
                target.canonicalPath
            }
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (error: AttachmentException) {
            throw error
        } catch (error: Exception) {
            throw AttachmentException(AttachmentError.STORAGE, error)
        } finally {
            if (!moved) deleteStaging(staged.file)
        }
    }

    /** Borrows the terminal's authenticated SFTP channel; never opens another SSH login. */
    suspend fun uploadSsh(context: Context, uri: Uri, files: SftpClient): String {
        val stagingRoot = withContext(Dispatchers.IO) {
            try {
                File(context.cacheDir, "pebrel-attachments")
            } catch (error: Exception) {
                throw AttachmentException(AttachmentError.STORAGE, error)
            }
        }
        val staged = withTimeout(STAGE_TIMEOUT_MS) {
            stageSource(context.contentResolver, uri, stagingRoot)
        }
        return try {
            uploadStaged(files, staged)
        } finally {
            deleteStaging(staged.file)
        }
    }

    private suspend fun stageSource(
        resolver: ContentResolver,
        uri: Uri,
        stagingDirectory: File,
    ): StagedAttachment = withContext(Dispatchers.IO) {
        val metadata = sourceMetadata(resolver, uri)
        currentCoroutineContext().ensureActive()
        if (metadata.size != null && metadata.size > MAX_BYTES) {
            throw AttachmentException(AttachmentError.TOO_LARGE)
        }
        ensureDirectory(stagingDirectory)
        val file = stagingDirectory.resolve(".attachment-${UUID.randomUUID()}.part")
        try {
            val total = copySourceCancellable(resolver, uri, file)
            StagedAttachment(file, safeFileName(metadata.name), total)
        } catch (cancelled: CancellationException) {
            deleteQuietly(file)
            throw cancelled
        } catch (error: AttachmentException) {
            deleteQuietly(file)
            throw error
        } catch (error: Exception) {
            deleteQuietly(file)
            throw AttachmentException(AttachmentError.STORAGE, error)
        }
    }

    private suspend fun copySourceCancellable(
        resolver: ContentResolver,
        uri: Uri,
        file: File,
    ): Long = withContext(Dispatchers.IO) {
        suspendCancellableCoroutine { continuation ->
            val source = AtomicReference<InputStream?>(null)
            val destination = AtomicReference<FileOutputStream?>(null)
            continuation.invokeOnCancellation {
                runCatching { source.getAndSet(null)?.close() }
                runCatching { destination.getAndSet(null)?.close() }
            }
            try {
                source.set(try {
                    resolver.openInputStream(uri)
                } catch (error: Exception) {
                    throw AttachmentException(AttachmentError.UNREADABLE, error)
                } ?: throw AttachmentException(AttachmentError.UNREADABLE))
                if (!continuation.isActive) {
                    runCatching { source.getAndSet(null)?.close() }
                    return@suspendCancellableCoroutine
                }
                destination.set(FileOutputStream(file))
                if (!continuation.isActive) {
                    runCatching { source.getAndSet(null)?.close() }
                    runCatching { destination.getAndSet(null)?.close() }
                    return@suspendCancellableCoroutine
                }
                var total = 0L
                source.get()!!.use { input ->
                    destination.get()!!.use { output ->
                        val buffer = ByteArray(COPY_BUFFER_BYTES)
                        while (true) {
                            continuation.context.ensureActive()
                            val count = input.read(buffer)
                            if (count < 0) break
                            if (count == 0) continue
                            total += count.toLong()
                            if (total > MAX_BYTES) throw AttachmentException(AttachmentError.TOO_LARGE)
                            output.write(buffer, 0, count)
                        }
                        output.flush()
                        buffer.fill(0)
                    }
                }
                source.set(null)
                destination.set(null)
                if (continuation.isActive) runCatching { continuation.resume(total) }
            } catch (error: Throwable) {
                runCatching { source.getAndSet(null)?.close() }
                runCatching { destination.getAndSet(null)?.close() }
                if (continuation.isActive) runCatching { continuation.resumeWithException(error) }
            }
        }
    }

    private suspend fun uploadStaged(files: SftpClient, staged: StagedAttachment): String =
        withTimeout(TRANSFER_TIMEOUT_MS) {
            var directory: String? = null
            var committed = false
            try {
                // STAT resolves the SFTP home without opening a listing cursor or sending shell input.
                var root = files.stat(".").also { check(it.directory) }.path
                for (name in listOf(".cache", "pebrel", "attachments")) {
                    val path = sftpChild(root, name)
                    val entry = try { files.stat(path) } catch (missing: NativeSshException) {
                        if (missing.code != "SFTP_NOT_FOUND") throw missing
                        try { files.mkdir(path) } catch (race: NativeSshException) {
                            // Some SFTP v3 servers report a racing mkdir as a generic failure.
                            if (!runCatching { files.stat(path).directory }.getOrDefault(false)) throw race
                        }
                        files.stat(path)
                    }
                    check(entry.directory)
                    root = entry.path
                }
                val targetDirectory = sftpChild(root, UUID.randomUUID().toString())
                files.mkdir(targetDirectory)
                directory = targetDirectory
                val target = sftpChild(targetDirectory, staged.safeName)
                val path = withContext(Dispatchers.IO) {
                    FileInputStream(staged.file).use { files.upload(target, it, staged.bytes) }
                }
                if (path != target || !path.startsWith("/") || path.length > MAX_REMOTE_PATH_CHARS ||
                    path.any(Char::isISOControl)) throw AttachmentException(AttachmentError.INVALID_RESULT)
                committed = true
                path
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (error: AttachmentException) {
                throw error
            } catch (error: Exception) {
                throw transferFailure(error)
            } finally {
                // SFTP upload releases its own temporary file; never close the terminal connection.
                if (!committed) directory?.let { path ->
                    withContext(NonCancellable) { runCatching { files.remove(files.stat(path)) } }
                }
            }
        }

    private suspend fun sourceMetadata(resolver: ContentResolver, uri: Uri): SourceMetadata =
        withContext(Dispatchers.IO) {
            try {
                suspendCancellableCoroutine { continuation ->
                    val cancellation = CancellationSignal()
                    var cursor: Cursor? = null
                    continuation.invokeOnCancellation { cancellation.cancel(); runCatching { cursor?.close() } }
                    try {
                        cursor = resolver.query(
                            uri,
                            arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE),
                            null,
                            null,
                            null,
                            cancellation,
                        )
                        if (!continuation.isActive) {
                            cursor?.close()
                            return@suspendCancellableCoroutine
                        }
                        val result = cursor?.use { row ->
                            if (!row.moveToFirst()) return@use SourceMetadata(null, null)
                            val name = row.getColumnIndex(OpenableColumns.DISPLAY_NAME).let { index ->
                                if (index >= 0) row.getString(index) else null
                            }
                            val size = row.getColumnIndex(OpenableColumns.SIZE).let { index ->
                                if (index >= 0 && !row.isNull(index)) {
                                    row.getLong(index).takeIf { it >= 0L }
                                } else {
                                    null
                                }
                            }
                            SourceMetadata(name, size)
                        } ?: SourceMetadata(null, null)
                        cursor = null
                        if (continuation.isActive) runCatching { continuation.resume(result) }
                    } catch (error: Throwable) {
                        cursor?.close()
                        if (continuation.isActive) runCatching { continuation.resumeWithException(error) }
                    }
                }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                SourceMetadata(null, null)
            }
        }

    private fun safeFileName(candidate: String?): String {
        val raw = candidate.orEmpty().substringAfterLast('/').substringAfterLast('\\').trim()
        if (raw.isEmpty() || raw == "." || raw == "..") return "attachment"
        val dot = raw.lastIndexOf('.').takeIf { it > 0 && it < raw.lastIndex }
        val stem = sanitizeNamePart(if (dot == null) raw else raw.substring(0, dot))
        val extension = if (dot == null) "" else sanitizeNamePart(raw.substring(dot))
        val safeStem = stem.ifEmpty { "attachment" }
        val safeExtension = takeUtf8Bytes(extension, 80)
        val available = (MAX_FILE_NAME_BYTES - safeExtension.toByteArray(StandardCharsets.UTF_8).size).coerceAtLeast(1)
        val boundedStem = takeUtf8Bytes(safeStem, available)
        val result = boundedStem + safeExtension
        return result.takeIf { it != "." && it != ".." && it.isNotEmpty() } ?: "attachment"
    }

    private fun sanitizeNamePart(value: String): String = buildString(value.length) {
        value.forEach { character ->
            when {
                character.code < 0x20 || character.code == 0x7f -> append('_')
                character == '/' || character == '\\' || character == ':' -> append('_')
                else -> append(character)
            }
        }
    }.trim()

    private fun uniqueTarget(directory: File, safeName: String): File {
        val dot = safeName.lastIndexOf('.').takeIf { it > 0 }
        val stem = if (dot == null) safeName else safeName.substring(0, dot)
        val extension = if (dot == null) "" else safeName.substring(dot)
        repeat(8) {
            val suffix = "-${UUID.randomUUID().toString().replace("-", "")}"
            val extensionBytes = extension.toByteArray(StandardCharsets.UTF_8).size
            val maxStem = (MAX_FILE_NAME_BYTES - suffix.length - extensionBytes).coerceAtLeast(1)
            val name = takeUtf8Bytes(stem, maxStem) + suffix + extension
            val target = directory.resolve(name)
            if (!target.exists()) return target
        }
        throw AttachmentException(AttachmentError.STORAGE)
    }

    private fun moveWithoutOverwrite(source: File, target: File) {
        try {
            Files.move(source.toPath(), target.toPath(), StandardCopyOption.ATOMIC_MOVE)
        } catch (_: AtomicMoveNotSupportedException) {
            Files.move(source.toPath(), target.toPath())
        }
    }

    private fun ensureDirectory(directory: File) {
        if (directory.exists()) {
            if (!directory.isDirectory || !directory.canWrite()) throw AttachmentException(AttachmentError.STORAGE)
            return
        }
        if (!directory.mkdirs() && !directory.isDirectory) throw AttachmentException(AttachmentError.STORAGE)
        if (!directory.canWrite()) throw AttachmentException(AttachmentError.STORAGE)
    }

    private fun deleteQuietly(file: File) {
        runCatching {
            if (file.exists()) Files.deleteIfExists(file.toPath())
        }
    }

    private suspend fun deleteStaging(file: File) {
        withContext(NonCancellable + Dispatchers.IO) { deleteQuietly(file) }
    }

    private fun takeUtf8Bytes(value: String, maxBytes: Int): String {
        if (maxBytes <= 0 || value.isEmpty()) return ""
        val result = StringBuilder()
        var offset = 0
        var used = 0
        while (offset < value.length) {
            val codePoint = value.codePointAt(offset)
            val width = Character.charCount(codePoint)
            val piece = value.substring(offset, offset + width)
            val bytes = piece.toByteArray(StandardCharsets.UTF_8).size
            if (used + bytes > maxBytes) break
            result.append(piece)
            used += bytes
            offset += width
        }
        return result.toString()
    }

    private fun transferFailure(error: Throwable): AttachmentException =
        AttachmentException(AttachmentError.TRANSFER, error, sshFailureKind(error))

    private fun sshFailureKind(error: Throwable?): SshFailureKind? =
        generateSequence(error) { it.cause }.firstNotNullOfOrNull {
            when (it) {
                is SshFailure -> it.kind
                is NativeSshException -> nativeSshFailureKind(it)
                else -> null
            }
        }

    private fun nativeSshFailureKind(error: NativeSshException): SshFailureKind? =
        SshFailureKind.entries.firstOrNull { it.name == error.code }

    private data class SourceMetadata(val name: String?, val size: Long?)

    private data class StagedAttachment(val file: File, val safeName: String, val bytes: Long)

}
