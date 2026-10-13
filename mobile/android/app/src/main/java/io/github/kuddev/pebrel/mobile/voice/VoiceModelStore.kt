package io.github.kuddev.pebrel.mobile.voice

import android.content.Context
import io.github.kuddev.pebrel.voice.LocalWhisper
import kotlinx.coroutines.*
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.io.IOException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.MessageDigest
import java.util.concurrent.TimeUnit

internal interface VoiceBackend {
    suspend fun hasModel(): Boolean
    suspend fun download(progress: (Float) -> Unit)
    suspend fun remove()
    suspend fun transcribe(samples: FloatArray): String
}

/** Download is an explicit UI action. No constructor/startup path performs network IO. */
internal class VoiceModelStore(context: Context) : VoiceBackend {
    private val directory = File(context.noBackupFilesDir, "voice")
    private val model = File(directory, "ggml-base.bin")
    companion object {
        const val MODEL_BYTES = 147951465L
        const val MODEL_SHA256 = "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe"
        private const val URL = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin"
        private val ownership = Mutex()
        private val client = OkHttpClient.Builder().connectTimeout(15, TimeUnit.SECONDS)
            .readTimeout(20, TimeUnit.SECONDS).callTimeout(10, TimeUnit.MINUTES).build()
    }

    override suspend fun hasModel(): Boolean = withContext(Dispatchers.IO) {
        model.isFile && model.length() == MODEL_BYTES
    }

    override suspend fun download(progress: (Float) -> Unit): Unit = ownership.withLock {
        coroutineScope {
            val call = client.newCall(Request.Builder().url(URL).build())
            val cancellation = launch(start = CoroutineStart.UNDISPATCHED) {
                try { awaitCancellation() } finally { call.cancel() }
            }
            try {
                withContext(Dispatchers.IO) {
                    if (!directory.isDirectory && !directory.mkdirs()) throw IOException("model_storage")
                    val temporary = File.createTempFile("base-", ".part", directory)
                    try {
                        val digest = MessageDigest.getInstance("SHA-256")
                        call.execute().use { response ->
                            if (!response.isSuccessful) throw IOException("model_download")
                            val body = response.body ?: throw IOException("model_download")
                            val advertised = body.contentLength()
                            if (advertised != -1L && advertised != MODEL_BYTES) throw IOException("model_size")
                            var received = 0L
                            var lastPercent = -1
                            temporary.outputStream().use { output ->
                                body.byteStream().use { input ->
                                    val buffer = ByteArray(64 * 1024)
                                    while (true) {
                                        ensureActive()
                                        val count = input.read(buffer)
                                        if (count < 0) break
                                        received += count
                                        if (received > MODEL_BYTES) throw IOException("model_size")
                                        output.write(buffer, 0, count)
                                        digest.update(buffer, 0, count)
                                        val percent = (received * 100 / MODEL_BYTES).toInt()
                                        if (percent != lastPercent) { lastPercent = percent; progress(received.toFloat() / MODEL_BYTES) }
                                    }
                                }
                                output.fd.sync()
                            }
                            if (received != MODEL_BYTES || digest.hex() != MODEL_SHA256) throw IOException("model_checksum")
                        }
                        ensureActive()
                        Files.move(temporary.toPath(), model.toPath(), StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
                        Unit
                    } finally { temporary.delete() }
                }
            } finally { cancellation.cancel() }
        }
    }

    override suspend fun remove() = ownership.withLock {
        withContext(Dispatchers.IO) {
            // Only this one app-owned model. Never recurse or remove a user-selected directory.
            if (model.exists() && !model.delete()) throw IOException("model_remove")
        }
    }

    override suspend fun transcribe(samples: FloatArray): String = ownership.withLock {
        withContext(Dispatchers.IO) {
            if (!hasModel()) throw IOException("model_missing")
            val digest = MessageDigest.getInstance("SHA-256")
            model.inputStream().use { input ->
                val buffer = ByteArray(64 * 1024)
                while (true) {
                    ensureActive()
                    val count = input.read(buffer)
                    if (count < 0) break
                    digest.update(buffer, 0, count)
                }
            }
            if (digest.hex() != MODEL_SHA256) throw IOException("model_checksum")
            LocalWhisper.transcribe(model, samples)
        }
    }

    private fun MessageDigest.hex() = digest().joinToString("") { "%02x".format(it) }
}
