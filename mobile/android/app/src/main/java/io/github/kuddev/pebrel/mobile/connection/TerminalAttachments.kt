package io.github.kuddev.pebrel.mobile.connection

import android.content.ContentResolver
import android.content.Context
import android.database.Cursor
import android.net.Uri
import android.os.CancellationSignal
import android.provider.OpenableColumns
import android.util.Base64
import io.github.kuddev.pebrel.mobile.session.LocalTerminalStorage
import io.github.kuddev.pebrel.ssh.NativeSshException
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import java.io.ByteArrayOutputStream
import java.io.Closeable
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
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
    REMOTE_TOOLS,
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

    private const val MAX_OUTPUT_BYTES = 16 * 1024
    private const val MAX_FILE_NAME_BYTES = 240
    private const val MAX_REMOTE_PATH_CHARS = 4096
    private const val STAGE_TIMEOUT_MS = 5 * 60_000L
    private const val CONNECT_TIMEOUT_MS = 30_000L
    private const val TRANSFER_TIMEOUT_MS = 5 * 60_000L
    private const val COPY_BUFFER_BYTES = 16 * 1024
    private const val BASE64_BUFFER_BYTES = 24 * 1024

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

    /**
     * Uploads a SAF document through one independent SSH exec session and returns the
     * absolute path printed by the remote shell after the file is committed.
     */
    suspend fun uploadSsh(
        context: Context,
        uri: Uri,
        host: HostProfile,
        password: CharArray,
        verify: (HostProfile, String) -> Boolean,
    ): String {
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
            uploadStaged(host, password, verify, staged)
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

    private suspend fun uploadStaged(
        host: HostProfile,
        password: CharArray,
        verify: (HostProfile, String) -> Boolean,
        staged: StagedAttachment,
    ): String = withTimeout(TRANSFER_TIMEOUT_MS) {
        val ssh = UploadSsh(host, password, verify)
        try {
            val connection = ssh.open()
            try {
                val encodedLength = encodedLength(staged.bytes)
                val marker = "PEBREL_ATTACHMENT_END_${UUID.randomUUID().toString().replace("-", "")}"
                val plan = uploadCommand(staged.safeName, staged.bytes, encodedLength, marker)
                ssh.blocking(connection) { connection.openExec(plan.command) }

                val stdout = connection.input(false)
                val stderr = connection.input(true)
                val result = coroutineScope {
                    val output: Deferred<String> = async(Dispatchers.IO) {
                        readBoundedOrClose(connection, stdout)
                    }
                    val diagnostics: Deferred<String> = async(Dispatchers.IO) {
                        readBoundedOrClose(connection, stderr)
                    }
                    val writer: Deferred<Unit> = async(Dispatchers.IO) {
                        try {
                            ssh.blocking(connection) {
                                FileInputStream(staged.file).use { source ->
                                    val sent = writeBase64(source, connection.output())
                                    check(sent == encodedLength)
                                    connection.output().write(marker.toByteArray(StandardCharsets.UTF_8))
                                    connection.output().flush()
                                }
                            }
                        } catch (error: Throwable) {
                            connection.close()
                            throw error
                        }
                    }
                    val exit: Deferred<Int> = async(Dispatchers.IO) {
                        try {
                            ssh.blocking(connection) { connection.awaitExit() }
                        } catch (error: Throwable) {
                            connection.close()
                            throw error
                        }
                    }

                    val exitCode = exit.await()
                    if (exitCode != 0 && writer.isActive) {
                        connection.close()
                        writer.cancel()
                    }
                    val writerFailure = try {
                        writer.await()
                        null
                    } catch (cancelled: CancellationException) {
                        currentCoroutineContext().ensureActive()
                        cancelled
                    } catch (error: Exception) {
                        error
                    }
                    val outputText = awaitOutputOrEmpty(output)
                    val diagnosticsText = awaitOutputOrEmpty(diagnostics)
                    RemoteCommandResult(exitCode, outputText, diagnosticsText, writerFailure)
                }
                if (result.exitCode != 0) throw remoteFailure(result)
                result.writerFailure?.let { throw transferFailure(it) }
                parseRemotePath(result.stdout, staged.safeName, plan.randomDirectory)
            } finally {
                ssh.release(connection)
            }
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (error: AttachmentException) {
            throw error
        } catch (error: SshFailure) {
            throw AttachmentException(AttachmentError.TRANSFER, error, error.kind)
        } catch (error: NativeSshException) {
            throw AttachmentException(AttachmentError.TRANSFER, error, nativeSshFailureKind(error))
        } catch (error: Exception) {
            throw transferFailure(error)
        } finally {
            ssh.close()
        }
    }

    private fun uploadCommand(
        fileName: String,
        sourceLength: Long,
        encodedLength: Long,
        marker: String,
    ): RemoteUploadPlan {
        val random = UUID.randomUUID().toString().replace("-", "")
        val markerBytes = marker.toByteArray(StandardCharsets.UTF_8).size
        val quotedName = shellQuote(fileName)
        val quotedRandom = shellQuote(random)
        val quotedMarker = shellQuote(marker)
        val dollar = "${'$'}"
        val command = """
            set -eu
            umask 077
            if ! command -v head >/dev/null 2>&1; then
                printf '%s\n' 'tool_missing=head' >&2
                exit 71
            fi
            if ! command -v base64 >/dev/null 2>&1; then
                printf '%s\n' 'tool_missing=base64' >&2
                exit 72
            fi
            if ! command -v wc >/dev/null 2>&1; then
                printf '%s\n' 'tool_missing=wc' >&2
                exit 73
            fi
            home=${dollar}{HOME:-}
            if [ -z "${dollar}home" ]; then
                printf '%s\n' 'home_missing' >&2
                exit 74
            fi
            home=${dollar}(cd "${dollar}home" 2>/dev/null && pwd -P) || exit 75
            root="${dollar}home/.cache/pebrel/attachments"
            directory="${dollar}root"/$quotedRandom
            target="${dollar}directory"/$quotedName
            temporary="${dollar}directory/.upload-$random"
            mkdir -p "${dollar}root"
            if ! mkdir "${dollar}directory"; then
                printf '%s\n' 'directory_create_failed' >&2
                exit 76
            fi
            cleanup() { rm -rf "${dollar}directory"; }
            trap cleanup 0 1 2 3 15
            if ! head -c $encodedLength | base64 -d > "${dollar}temporary"; then
                printf '%s\n' 'upload_decode_failed' >&2
                exit 77
            fi
            decoded=${dollar}(wc -c < "${dollar}temporary")
            if [ "${dollar}decoded" -ne $sourceLength ]; then
                printf '%s\n' 'upload_size_mismatch' >&2
                exit 78
            fi
            confirmation=${dollar}(head -c $markerBytes)
            if [ "${dollar}confirmation" != $quotedMarker ]; then
                printf '%s\n' 'upload_incomplete' >&2
                exit 79
            fi
            mv "${dollar}temporary" "${dollar}target"
            printf 'PEBREL_ATTACHMENT=%s\n' "${dollar}target"
            trap - 0 1 2 3 15
        """.trimIndent()
        return RemoteUploadPlan(command, random)
    }

    private fun writeBase64(input: InputStream, output: OutputStream): Long {
        val buffer = ByteArray(BASE64_BUFFER_BYTES + 2)
        var carry = 0
        var encodedBytes = 0L
        while (true) {
            val count = input.read(buffer, carry, BASE64_BUFFER_BYTES)
            if (count < 0) break
            if (count == 0) continue
            val total = carry + count
            val complete = total / 3 * 3
            if (complete > 0) {
                val encoded = Base64.encode(buffer, 0, complete, Base64.NO_WRAP)
                output.write(encoded)
                encodedBytes += encoded.size.toLong()
                encoded.fill(0)
            }
            carry = total - complete
            if (carry > 0) System.arraycopy(buffer, complete, buffer, 0, carry)
        }
        if (carry > 0) {
            val encoded = Base64.encode(buffer, 0, carry, Base64.NO_WRAP)
            output.write(encoded)
            encodedBytes += encoded.size.toLong()
            encoded.fill(0)
        }
        buffer.fill(0)
        return encodedBytes
    }

    private fun readBoundedOrClose(connection: SshConnection, input: InputStream): String {
        return try {
            readBounded(input)
        } catch (error: Throwable) {
            connection.close()
            throw error
        }
    }

    private suspend fun awaitOutputOrEmpty(output: Deferred<String>): String = try {
        output.await()
    } catch (cancelled: CancellationException) {
        currentCoroutineContext().ensureActive()
        ""
    } catch (_: Exception) {
        ""
    }

    private fun readBounded(input: InputStream): String {
        val output = ByteArrayOutputStream(minOf(MAX_OUTPUT_BYTES, 4096))
        val buffer = ByteArray(COPY_BUFFER_BYTES)
        var remaining = MAX_OUTPUT_BYTES
        while (true) {
            val count = input.read(buffer)
            if (count < 0) break
            if (remaining > 0) {
                val accepted = minOf(remaining, count)
                output.write(buffer, 0, accepted)
                remaining -= accepted
            }
        }
        return String(output.toByteArray(), StandardCharsets.UTF_8)
    }

    private fun remoteFailure(result: RemoteCommandResult): AttachmentException {
        val toolMissing = result.stderr.lineSequence().firstOrNull { it.startsWith("tool_missing=") }
        return if (toolMissing != null) {
            AttachmentException(AttachmentError.REMOTE_TOOLS)
        } else {
            AttachmentException(AttachmentError.TRANSFER)
        }
    }

    private fun parseRemotePath(stdout: String, fileName: String, randomDirectory: String): String {
        val prefix = "PEBREL_ATTACHMENT="
        val path = stdout.lineSequence().firstOrNull { it.startsWith(prefix) }
            ?.removePrefix(prefix)
            ?.trim()
            ?: throw AttachmentException(AttachmentError.INVALID_RESULT)
        val directoryMarker = "/.cache/pebrel/attachments/$randomDirectory/"
        if (!path.startsWith("/") || path.length > MAX_REMOTE_PATH_CHARS ||
            path.any { it.code < 0x20 || it.code == 0x7f } ||
            !path.contains(directoryMarker) || !path.endsWith("/$fileName")) {
            throw AttachmentException(AttachmentError.INVALID_RESULT)
        }
        return path
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

    private fun encodedLength(bytes: Long): Long = ((bytes + 2L) / 3L) * 4L

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

    private fun shellQuote(value: String): String = "'${value.replace("'", "'\"'\"'")}'"

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

    private data class RemoteCommandResult(
        val exitCode: Int,
        val stdout: String,
        val stderr: String,
        val writerFailure: Throwable?,
    )

    private data class RemoteUploadPlan(val command: String, val randomDirectory: String)

    /** One cancellation-aware native connection for this upload only. */
    private class UploadSsh(
        private val host: HostProfile,
        password: CharArray,
        private val verify: (HostProfile, String) -> Boolean,
    ) : Closeable {
        private val initialPassword = password.copyOf()
        private val guard = Any()
        @Volatile private var active: SshConnection? = null
        @Volatile private var closed = false

        suspend fun open(): SshConnection {
            val connection = synchronized(guard) {
                check(!closed) { "closed" }
                SshConnection(host, initialPassword.copyOf(), verify).also { active = it }
            }
            return try {
                withTimeout(CONNECT_TIMEOUT_MS) {
                    blocking(connection) { connection.connect() }
                }
                initialPassword.fill('\u0000')
                connection
            } catch (error: Throwable) {
                release(connection)
                initialPassword.fill('\u0000')
                throw error
            }
        }

        suspend fun <T> blocking(connection: SshConnection, action: () -> T): T =
            withContext(Dispatchers.IO) {
                suspendCancellableCoroutine { continuation ->
                    continuation.invokeOnCancellation { connection.close() }
                    try {
                        continuation.resume(action())
                    } catch (error: Throwable) {
                        continuation.resumeWithException(error)
                    }
                }
            }

        fun release(connection: SshConnection) {
            synchronized(guard) {
                if (active === connection) active = null
            }
            connection.close()
        }

        override fun close() {
            val connection = synchronized(guard) {
                closed = true
                active.also { active = null }
            }
            initialPassword.fill('\u0000')
            connection?.close()
        }
    }
}
