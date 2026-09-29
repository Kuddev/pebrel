package io.github.kuddev.pebrel.voice

import kotlinx.coroutines.*
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.io.File

/** One inference at a time; no model or worker is resident while voice input is idle. */
object LocalWhisper {
    private val inference = Mutex()

    suspend fun transcribe(model: File, pcm: FloatArray): String = inference.withLock {
        require(pcm.size in 4800..960000)
        currentCoroutineContext().ensureActive()
        val handle = NativeWhisper.create()
        check(handle != 0L)
        try {
            coroutineScope {
                val cancellation = launch(start = CoroutineStart.UNDISPATCHED) {
                    try { awaitCancellation() } finally { NativeWhisper.cancel(handle) }
                }
                try {
                    withContext(Dispatchers.IO) {
                        NativeWhisper.transcribe(handle, model.absolutePath, pcm).toString(Charsets.UTF_8).trim()
                    }
                } finally { cancellation.cancel() }
            }
        } finally { NativeWhisper.destroy(handle) }
    }
}

internal object NativeWhisper {
    init { System.loadLibrary("pebrel_voice") }
    external fun create(): Long
    external fun cancel(handle: Long)
    external fun destroy(handle: Long)
    external fun transcribe(handle: Long, model: String, samples: FloatArray): ByteArray
}
