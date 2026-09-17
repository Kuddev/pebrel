package io.github.kuddev.pebrel.mobile.voice

import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

internal enum class VoicePhase { Checking, Missing, Ready, Downloading, Recording, Transcribing }
internal enum class VoiceError { Permission, Failed, Empty, Short, Model, Engine }
internal data class VoiceState(val phase: VoicePhase = VoicePhase.Checking, val error: VoiceError? = null,
    val progress: Float = 0f, val level: Float = 0f, val seconds: Int = 0, val cancelArmed: Boolean = false)

/** Owned by one composer/session. Never knows how to execute or send a command. */
internal class VoiceInputController(
    private val scope: CoroutineScope,
    private val backend: VoiceBackend,
    private val recorder: VoiceRecorder = PcmRecorder,
    private val onTranscript: (String) -> Unit,
) {
    private val mutable = MutableStateFlow(VoiceState())
    val state = mutable.asStateFlow()
    @Volatile private var generation = 0L
    private var job: Job? = null
    private var modelReady = false
    @Volatile private var holding = false

    init { refresh() }

    fun refresh() {
        cancel()
        val token = generation
        mutable.value = VoiceState(VoicePhase.Checking)
        job = scope.launch {
            val ready = runCatching { backend.hasModel() }.getOrDefault(false)
            ensureActive()
            if (generation == token) { modelReady = ready; mutable.value = idle() }
        }
    }
    private fun idle(error: VoiceError? = null) = VoiceState(if (modelReady) VoicePhase.Ready else VoicePhase.Missing, error)

    fun download() {
        if (state.value.phase !in setOf(VoicePhase.Missing, VoicePhase.Ready)) return
        val token = ++generation
        mutable.value = VoiceState(VoicePhase.Downloading)
        job = scope.launch {
            try {
                backend.download { progress -> if (generation == token) mutable.update { it.copy(progress = progress) } }
                ensureActive()
                if (generation == token) { modelReady = true; mutable.value = idle() }
            } catch (error: CancellationException) { throw error }
            catch (_: Exception) { if (generation == token) mutable.value = idle(VoiceError.Model) }
        }
    }

    fun remove() {
        if (state.value.phase != VoicePhase.Ready) return
        val token = ++generation
        mutable.value = VoiceState(VoicePhase.Checking)
        job = scope.launch {
            try {
                backend.remove()
                ensureActive()
                if (generation == token) { modelReady = false; mutable.value = idle() }
            } catch (error: CancellationException) { throw error }
            catch (_: Exception) { if (generation == token) mutable.value = idle(VoiceError.Model) }
        }
    }

    fun permissionDenied() { mutable.value = idle(VoiceError.Permission) }

    fun begin(): Boolean {
        if (state.value.phase != VoicePhase.Ready) return false
        val token = ++generation
        holding = true
        mutable.value = VoiceState(VoicePhase.Recording)
        job = scope.launch {
            var audio: FloatArray? = null
            try {
                audio = recorder.capture({ holding && generation == token }) { level, seconds ->
                    if (generation == token) mutable.update { it.copy(level = level, seconds = seconds) }
                }
                ensureActive()
                if (generation != token) return@launch
                if (state.value.cancelArmed) { mutable.value = idle(); return@launch }
                if (audio.size < 4800) { mutable.value = idle(VoiceError.Short); return@launch }
                if (audio.none { kotlin.math.abs(it) > .008f }) { mutable.value = idle(VoiceError.Empty); return@launch }
                holding = false
                mutable.value = VoiceState(VoicePhase.Transcribing)
                val text = withTimeout(120_000) { backend.transcribe(audio) }.trim()
                ensureActive()
                if (generation == token) {
                    mutable.value = idle(if (text.isBlank()) VoiceError.Empty else null)
                    if (text.isNotBlank()) onTranscript(text)
                }
            } catch (_: TimeoutCancellationException) { if (generation == token) mutable.value = idle(VoiceError.Failed) }
            catch (error: CancellationException) { throw error }
            catch (_: LinkageError) { if (generation == token) mutable.value = idle(VoiceError.Engine) }
            catch (_: SecurityException) { if (generation == token) mutable.value = idle(VoiceError.Permission) }
            catch (_: Exception) { if (generation == token) mutable.value = idle(VoiceError.Failed) }
            finally { audio?.fill(0f); if (generation == token) holding = false }
        }
        return true
    }

    fun drag(cancel: Boolean) {
        if (state.value.phase == VoicePhase.Recording) mutable.update { it.copy(cancelArmed = cancel) }
    }
    fun release() {
        if (state.value.phase != VoicePhase.Recording) return
        if (state.value.cancelArmed) cancel() else holding = false
    }
    fun cancel() {
        generation++
        holding = false
        job?.cancel()
        mutable.value = idle()
    }
}

/** Preserve the latest draft, reject overflow, and never interpret transcription as input keys. */
internal fun appendVoiceDraft(draft: String, spoken: String, limit: Int = 8192): String? {
    val text = spoken.trim()
    if (text.isEmpty()) return draft
    val result = draft + (if (draft.isNotEmpty() && !draft.last().isWhitespace()) " " else "") + text
    return result.takeIf { it.length <= limit }
}
