package io.github.kuddev.pebrel.mobile.session

import android.os.Looper
import androidx.test.core.app.ApplicationProvider
import io.github.kuddev.pebrel.mobile.connection.*
import kotlinx.coroutines.*
import kotlinx.coroutines.test.*
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@OptIn(ExperimentalCoroutinesApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class DesktopRecoveryTest {
    private class Link : DesktopTransport {
        lateinit var receive: (JSONObject) -> Unit
        lateinit var lost: (Throwable?) -> Unit
        var closed = false
        val methods = java.util.concurrent.CopyOnWriteArrayList<String>()
        override suspend fun open(allowInput: Boolean, receive: (JSONObject) -> Unit, disconnected: (Throwable?) -> Unit) {
            this.receive = receive
            lost = disconnected
            receive(JSONObject().put("type", "mobile.ready").put("protocol", "pebrel.mobile.relay").put("version", 1)
                .put("capabilities", JSONObject().put("input", true)))
        }
        override fun send(frame: JSONObject) {
            val method = frame.getString("method")
            methods += method
            receive(JSONObject().put("id", frame.getString("id")).put("ok", true).put("result", JSONObject()))
            if (method == "events.subscribe") receive(JSONObject("""{
                "event":"runtime.snapshot","data":{"process_id":11,"windows":[{"id":1,"tabs":[
                {"label":"Project","panes":[{"id":2,"title":"Shell","task_state":"idle"}]}]}]}}
            """))
        }
        override fun close() { closed = true }
    }

    private suspend fun TestScope.awaitState(condition: () -> Boolean) {
        repeat(200) {
            shadowOf(Looper.getMainLooper()).idle()
            runCurrent()
            if (condition()) return
            withContext(Dispatchers.IO) { delay(5) }
        }
        fail("repository did not reach expected state")
    }

    @Test fun foregroundRecoveryKeepsComputerPaneAndDraftAndDoesNotReplayInput() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val links = mutableListOf<Link>()
        val repository = SessionRepository(ApplicationProvider.getApplicationContext()) { Link().also(links::add) }
        try {
            repository.foregroundChanged(true)
            val id = repository.connectRelay(RelayProfile("wss://example.com", "pc", "a".repeat(43), "PC"))
            awaitState { repository.desktops.value.singleOrNull()?.status == "ready" }
            val first = repository.desktops.value.single()
            val target = "$id:1:2"
            repository.setDraft(target, "unsent draft")
            repository.foregroundChanged(false)
            links.first().lost(java.io.IOException("network gone"))
            awaitState { repository.desktops.value.single().status == "disconnected" }
            advanceTimeBy(60_000); runCurrent()
            assertEquals(1, links.size)
            assertEquals(first.panes, repository.desktops.value.single().panes)

            repository.foregroundChanged(true)
            advanceTimeBy(1); runCurrent()
            awaitState { repository.desktops.value.single().status == "ready" && links.size == 2 }
            val recovered = repository.desktops.value.single()
            assertEquals(id, recovered.id)
            assertEquals(first.panes, recovered.panes)
            assertEquals(first.connectionGeneration + 1, recovered.connectionGeneration)
            assertEquals("unsent draft", repository.drafts.value[target])
            links.first().lost(DesktopConnectionFailure(DesktopFailureKind.AUTHENTICATION))
            shadowOf(Looper.getMainLooper()).idle()
            assertEquals("ready", repository.desktops.value.single().status)
            assertTrue(links.all { link -> link.methods.none { it in setOf("pane.prompt", "pane.send_key") } })
            repository.closeDesktop(id)
            advanceTimeBy(60_000); runCurrent()
            assertTrue(repository.desktops.value.isEmpty())
            assertEquals(2, links.size)
        } finally { repository.foregroundChanged(false); repository.closeAll(); Dispatchers.resetMain() }
    }

    @Test fun successfulComputerWithChangedCertificateRequiresUserAction() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val links = mutableListOf<Link>()
        val repository = SessionRepository(ApplicationProvider.getApplicationContext()) { Link().also(links::add) }
        try {
            repository.foregroundChanged(true)
            repository.connectRelay(RelayProfile("wss://example.com", "pc", "a".repeat(43), "PC"))
            awaitState { repository.desktops.value.singleOrNull()?.status == "ready" }
            links.first().lost(DesktopConnectionFailure(DesktopFailureKind.CERTIFICATE_CHANGED))
            awaitState { repository.desktops.value.single().status == "disconnected" }
            repository.foregroundChanged(false)
            repository.foregroundChanged(true)
            advanceTimeBy(60_000); runCurrent()
            assertEquals(1, links.size)
            assertEquals(DesktopFailureKind.CERTIFICATE_CHANGED, repository.desktops.value.single().failure)
        } finally { repository.foregroundChanged(false); repository.closeAll(); Dispatchers.resetMain() }
    }
}
