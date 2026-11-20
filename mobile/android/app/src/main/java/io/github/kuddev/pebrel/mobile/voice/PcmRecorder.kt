package io.github.kuddev.pebrel.mobile.voice

import android.annotation.SuppressLint
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import kotlinx.coroutines.*
import java.io.IOException
import kotlin.math.sqrt

internal fun interface VoiceRecorder {
    suspend fun capture(holding: () -> Boolean, level: (Float, Int) -> Unit): FloatArray
}

internal object PcmRecorder : VoiceRecorder {
    @SuppressLint("MissingPermission") // Runtime grant checked at the UI boundary; revocation still fails closed.
    override suspend fun capture(holding: () -> Boolean, level: (Float, Int) -> Unit): FloatArray = withContext(Dispatchers.IO) {
        val minimum = AudioRecord.getMinBufferSize(16000, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        if (minimum <= 0) throw IOException("microphone_unavailable")
        val recorder = AudioRecord.Builder().setAudioSource(MediaRecorder.AudioSource.VOICE_RECOGNITION)
            .setAudioFormat(AudioFormat.Builder().setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                .setSampleRate(16000).setChannelMask(AudioFormat.CHANNEL_IN_MONO).build())
            .setBufferSizeInBytes(maxOf(minimum * 2, 6400)).build()
        val samples = FloatArray(16000 * 60)
        val buffer = ShortArray(800)
        var used = 0
        try {
            if (recorder.state != AudioRecord.STATE_INITIALIZED) throw IOException("microphone_unavailable")
            ensureActive()
            if (!holding()) return@withContext FloatArray(0)
            recorder.startRecording()
            if (recorder.recordingState != AudioRecord.RECORDSTATE_RECORDING) throw IOException("microphone_busy")
            while (holding() && used < samples.size) {
                ensureActive()
                val read = recorder.read(buffer, 0, minOf(buffer.size, samples.size - used), AudioRecord.READ_NON_BLOCKING)
                if (read < 0) throw IOException("microphone_read")
                if (read == 0) { delay(10); continue }
                var energy = 0f
                for (i in 0 until read) {
                    val value = buffer[i] / 32768f
                    samples[used++] = value
                    energy += value * value
                }
                level(sqrt(energy / read).coerceIn(0f, 1f), used / 16000)
            }
            samples.copyOf(used)
        } finally {
            runCatching { recorder.stop() }
            recorder.release()
            buffer.fill(0)
            samples.fill(0f)
        }
    }
}
