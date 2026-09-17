package io.github.kuddev.pebrel.mobile.voice

import kotlinx.coroutines.*
import kotlinx.coroutines.test.*
import org.junit.Assert.*
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class VoiceInputControllerTest {
    private class Backend(var installed: Boolean = true) : VoiceBackend {
        var downloads = 0
        var removals = 0
        var transcriptions = 0
        var pending: CompletableDeferred<String>? = null
        override suspend fun hasModel() = installed
        override suspend fun download(progress: (Float) -> Unit) { downloads++; progress(.5f); installed = true }
        override suspend fun remove() { removals++; installed = false }
        override suspend fun transcribe(samples: FloatArray): String {
            transcriptions++
            return pending?.let { withContext(NonCancellable) { it.await() } } ?: "echo hello"
        }
    }

    private fun recorder() = VoiceRecorder { holding, level ->
        while (holding()) { level(.2f, 1); delay(10) }
        FloatArray(6400) { .2f }
    }

    @Test fun startupAndPermissionDoNotDownloadOrRecord() = runTest {
        val backend = Backend(false)
        val controller = VoiceInputController(backgroundScope, backend, recorder()) { fail("unexpected transcript") }
        runCurrent()
        assertEquals(VoicePhase.Missing, controller.state.value.phase)
        assertFalse(controller.begin())
        controller.permissionDenied()
        runCurrent()
        assertEquals(0, backend.downloads)
        assertEquals(0, backend.transcriptions)
    }

    @Test fun modelInstallAndRemovalOnlyHappenAfterExplicitActions() = runTest {
        val backend = Backend(false)
        val controller = VoiceInputController(backgroundScope, backend, recorder()) {}
        runCurrent()
        controller.download()
        runCurrent()
        assertEquals(1, backend.downloads)
        assertEquals(VoicePhase.Ready, controller.state.value.phase)
        controller.remove()
        runCurrent()
        assertEquals(1, backend.removals)
        assertEquals(VoicePhase.Missing, controller.state.value.phase)
    }

    @Test fun holdDoesNotTranscribeUntilReleaseAndRejectsDuplicateStarts() = runTest {
        val backend = Backend()
        val results = mutableListOf<String>()
        val controller = VoiceInputController(backgroundScope, backend, recorder(), results::add)
        runCurrent()
        assertTrue(controller.begin())
        assertFalse(controller.begin())
        runCurrent()
        assertEquals(0, backend.transcriptions)
        controller.release()
        advanceTimeBy(20)
        runCurrent()
        assertEquals(listOf("echo hello"), results)
        assertEquals(VoicePhase.Ready, controller.state.value.phase)
    }

    @Test fun slideCancelAndLostLifecycleDiscardAudioWithoutRecognition() = runTest {
        val backend = Backend()
        val controller = VoiceInputController(backgroundScope, backend, recorder()) { fail("cancelled voice filled a draft") }
        runCurrent()
        controller.begin()
        runCurrent()
        controller.drag(true)
        controller.release()
        runCurrent()
        controller.begin()
        runCurrent()
        controller.cancel()
        runCurrent()
        assertEquals(0, backend.transcriptions)
    }

    @Test fun staleInferenceCannotFillDraftAfterCancelOrSessionDisposal() = runTest {
        val backend = Backend().apply { pending = CompletableDeferred() }
        val controller = VoiceInputController(backgroundScope, backend, recorder()) { fail("stale transcript") }
        runCurrent()
        controller.begin()
        runCurrent()
        controller.release()
        advanceTimeBy(20)
        runCurrent()
        assertEquals(VoicePhase.Transcribing, controller.state.value.phase)
        controller.cancel()
        backend.pending!!.complete("old session command")
        runCurrent()
        assertEquals(VoicePhase.Ready, controller.state.value.phase)
    }

    @Test fun permissionResultAloneDoesNotStartRecorder() = runTest {
        var captures = 0
        val controller = VoiceInputController(backgroundScope, Backend(), VoiceRecorder { _, _ ->
            captures++; FloatArray(0)
        }) {}
        runCurrent()
        controller.permissionDenied()
        controller.refresh()
        runCurrent()
        assertEquals(0, captures)
    }

    @Test fun tooShortAndSilentAudioNeverCallInference() = runTest {
        for (audio in listOf(FloatArray(100) { .2f }, FloatArray(6400))) {
            val backend = Backend()
            val controller = VoiceInputController(backgroundScope, backend, VoiceRecorder { _, _ -> audio }) { fail() }
            runCurrent()
            controller.begin()
            runCurrent()
            assertEquals(0, backend.transcriptions)
            assertNotNull(controller.state.value.error)
        }
    }

    @Test fun recognizedWordsAppendToLatestDraftAndNeverTruncate() {
        assertEquals("echo existing hello", appendVoiceDraft("echo existing", "hello"))
        assertEquals("echo\nhello", appendVoiceDraft("echo\n", "hello"))
        assertEquals("  hello", appendVoiceDraft("  ", "hello"))
        assertNull(appendVoiceDraft("x".repeat(8192), "hello"))
        assertEquals("echo existing", appendVoiceDraft("echo existing", " "))
    }
}
