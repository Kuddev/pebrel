package io.github.kuddev.pebrel.mobile.session

import android.os.Looper
import androidx.test.core.app.ApplicationProvider
import io.github.kuddev.pebrel.mobile.connection.*
import kotlinx.coroutines.*
import kotlinx.coroutines.test.*
import okhttp3.HttpUrl.Companion.toHttpUrl
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
    @Test fun conversationReadStartedBeforeAnAnswerCannotRestoreOldButtons() = runTest {
        fun page(prompt: Boolean) = JSONObject().put("identity", JSONObject().put("kind", "codex").put("session_id", "native").put("epoch", 4))
            .put("unchanged", false).put("messages", org.json.JSONArray()).put("revision", "revision")
            .put("before", JSONObject.NULL).put("can_send", !prompt).put("state", if (prompt) "waiting_input" else "finished")
            .put("prompt", if (prompt) JSONObject().put("id", "first-question").put("text", "Choose scope")
                .put("options", org.json.JSONArray(listOf("Current", "All"))).put("selected", 0) else JSONObject.NULL)
        val cache = ConversationCache()
        var pendingRead: CompletableDeferred<JSONObject>? = null
        val writes = mutableListOf<String>()
        val client = DesktopConversation(ConversationIdentity("codex", "native"), "view", cache,
            request = { method, _ ->
                if (method == "conversation.read") pendingRead?.await() ?: page(true)
                else { writes += method; JSONObject().put("accepted", true) }
            }, availability = { null })
        client.refresh()
        val prompt = checkNotNull(client.state.value.prompt)
        pendingRead = CompletableDeferred()
        val read = launch { client.refresh() }
        runCurrent()
        client.choose(prompt, 1)
        pendingRead!!.complete(page(true))
        read.join()
        assertNull(client.state.value.prompt)
        assertFalse(client.state.value.canSend)
        assertEquals(listOf("conversation.choose"), writes)
        assertTrue(runCatching { client.choose(prompt, 0) }.exceptionOrNull() is DesktopRpcFailure)
        val restored = checkNotNull(cache.get("view"))
        assertNull(restored.identity.epoch)
        assertNull(restored.prompt)
        assertFalse(restored.canSend)
    }

    @Test fun conversationKeyRechecksOwnershipAndNeverReplaysAfterAnUncertainReply() = runTest {
        var available: String? = null
        val sent = mutableListOf<String>()
        val client = DesktopConversation(ConversationIdentity("claude", "native"), "key-view", ConversationCache(),
            request = { method, params ->
                if (method == "conversation.read") JSONObject("""{"identity":{"kind":"claude","session_id":"native","epoch":8},"messages":[],"unchanged":false,"revision":"r","before":null,"can_send":true,"state":"finished","prompt":null}""")
                else { sent += params.getString("key"); throw java.io.IOException("response lost") }
            }, availability = { available })
        client.refresh()
        assertTrue(runCatching { client.key("Ctrl+C") }.isFailure)
        assertFalse(client.state.value.canSend)
        available = "conversation_identity_changed"
        assertTrue(runCatching { client.key("Esc") }.exceptionOrNull() is DesktopRpcFailure)
        assertEquals(listOf("Ctrl+C"), sent)
    }

    private class Link(private val autoSnapshot: Boolean = true, private val scrollSupported: Boolean = false, private val historySupported: Boolean = false) : DesktopTransport {
        lateinit var receive: (JSONObject) -> Unit
        lateinit var lost: (Throwable?) -> Unit
        var closed = false
        val methods = java.util.concurrent.CopyOnWriteArrayList<String>()
        val requests = java.util.concurrent.CopyOnWriteArrayList<JSONObject>()
        override suspend fun open(allowInput: Boolean, receive: (JSONObject) -> Unit, disconnected: (Throwable?) -> Unit) {
            this.receive = receive
            lost = disconnected
            receive(JSONObject().put("type", "mobile.ready").put("protocol", "pebrel.mobile.relay").put("version", 1)
                .put("capabilities", JSONObject().put("input", true).put("terminal_scroll", scrollSupported).put("terminal_history", historySupported)))
        }
        override fun send(frame: JSONObject) {
            val method = frame.getString("method")
            methods += method
            requests += JSONObject(frame.toString())
            receive(JSONObject().put("id", frame.getString("id")).put("ok", true).put("result", JSONObject()))
            if (method == "events.subscribe" && autoSnapshot) receive(JSONObject("""{
                "event":"runtime.snapshot","data":{"process_id":11,"windows":[{"id":1,"tabs":[
                {"label":"Project","panes":[{"id":2,"title":"Shell","task_state":"idle"}]}]}]}}
            """))
        }
        override fun close() { closed = true }
    }

    @Test fun negotiatedWheelReachesTheTransportAndRequestsTheControlledViewport() = runBlocking {
        val link = Link(scrollSupported = true)
        val client = DesktopRuntimeClient(link, {}, {})
        try {
            client.connect(true)
            assertTrue(client.terminalScrollSupported)
            client.dispatchInput("pane.scroll", JSONObject().put("window_id", 1).put("pane_id", 2)
                .put("lines", 3).put("column", 4).put("row", 5)).await()
            client.readPane(JSONObject().put("window_id", 1).put("pane_id", 2))
            val scroll = link.requests.single { it.getString("method") == "pane.scroll" }.getJSONObject("params")
            assertEquals(3, scroll.getInt("lines"))
            assertEquals(2, scroll.getInt("pane_id"))
            val read = link.requests.single { it.getString("method") == "pane.read" }.getJSONObject("params")
            assertTrue(read.getBoolean("screen"))
            assertTrue(read.getBoolean("screen_viewport"))
        } finally { client.close() }
    }

    @Test fun historyReaderNegotiatesReadOnlyPagesInsteadOfTheDesktopViewport() = runTest {
        val link = Link(scrollSupported = true, historySupported = true)
        val client = DesktopRuntimeClient(link, {}, {})
        try {
            client.connect(false)
            assertFalse(client.streamPane(JSONObject().put("window_id", 1).put("pane_id", 2)) { error("history is paged") })
            client.readPane(JSONObject().put("window_id", 1).put("pane_id", 2)
                .put("screen_history", JSONObject().put("start", 40).put("rows", 200)))
            val read = link.requests.single { it.getString("method") == "pane.read" }.getJSONObject("params")
            assertTrue(read.getBoolean("screen"))
            assertFalse(read.has("screen_viewport"))
            assertEquals(40L, read.getJSONObject("screen_history").getLong("start"))
            assertTrue(link.requests.none { it.getString("method") in setOf("pane.scroll", "pane.send_key", "pane.prompt") })
        } finally { client.close() }
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

    @Test fun networkChangeReplacesAnApparentlyLiveSocketWithoutReplayingInput() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val links = mutableListOf<Link>()
        val repository = SessionRepository(ApplicationProvider.getApplicationContext()) { Link().also(links::add) }
        try {
            repository.foregroundChanged(true)
            val id = repository.connectRelay(RelayProfile("wss://example.com", "pc", "a".repeat(43), "PC"))
            awaitState { repository.desktops.value.singleOrNull()?.status == "ready" }
            val first = repository.desktops.value.single()
            repository.setDraft("$id:1:2", "unsent")
            repository.networkChanged()
            advanceTimeBy(1); runCurrent()
            awaitState { links.size == 2 && links.first().closed && repository.desktops.value.single().status == "ready" }
            assertEquals(first.connectionGeneration + 1, repository.desktops.value.single().connectionGeneration)
            assertEquals("unsent", repository.drafts.value["$id:1:2"])
            assertTrue(links.all { link -> link.methods.none { it in setOf("pane.prompt", "pane.send_key") } })
            repository.foregroundChanged(false)
            repository.networkChanged()
            advanceTimeBy(60_000); runCurrent()
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
            repository.networkChanged()
            repository.foregroundChanged(false)
            repository.foregroundChanged(true)
            advanceTimeBy(60_000); runCurrent()
            assertEquals(1, links.size)
            assertEquals(DesktopFailureKind.CERTIFICATE_CHANGED, repository.desktops.value.single().failure)
        } finally { repository.foregroundChanged(false); repository.closeAll(); Dispatchers.resetMain() }
    }

    @Test fun scanningTheSameComputerAgainReusesItsWorkspaceAndSavedRecord() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val links = mutableListOf<Link>()
        val repository = SessionRepository(ApplicationProvider.getApplicationContext()) { Link().also(links::add) }
        fun profile(address: String, room: String) = RelayProfile(address, room, "a".repeat(43), "PC",
            "sha256/${"a".repeat(43)}=", "lan", SecureRelayProfile("b".repeat(43), "grant", "c".repeat(43), false))
        try {
            val original = profile("wss://192.0.2.1:4000", "old-room")
            val id = repository.connectRelay(original)
            awaitState { repository.desktops.value.singleOrNull()?.status == "ready" }
            val first = repository.desktops.value.single()
            repository.setDraft("$id:1:2", "draft stays with the computer")
            links.first().lost(java.io.IOException("network gone"))
            awaitState { repository.desktops.value.single().status == "disconnected" }
            val replacement = profile("wss://192.0.2.2:5000", "new-room")
            assertEquals(id, repository.connectRelay(replacement))
            awaitState { repository.desktops.value.singleOrNull()?.status == "ready" && links.size == 2 }
            assertEquals(first.connectionGeneration + 1, repository.desktops.value.single().connectionGeneration)
            assertEquals("draft stays with the computer", repository.drafts.value["$id:1:2"])
            assertSame(replacement, repository.relays.value.single())
            links.first().lost(DesktopConnectionFailure(DesktopFailureKind.AUTHENTICATION))
            shadowOf(Looper.getMainLooper()).idle()
            assertEquals("ready", repository.desktops.value.single().status)
            repository.closeDesktop(id)
            assertEquals(id, repository.restoreDesktop("relay:${repository.relays.value.single().legacyId}"))
            awaitState { repository.desktops.value.singleOrNull()?.status == "ready" && links.size == 3 }
            assertEquals(1, repository.relays.value.size)
        } finally { repository.closeAll(); Dispatchers.resetMain() }
    }

    @Test fun aNewInvitationReplacesAStalledLinkWithoutOverwritingSavedCredentials() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val links = mutableListOf<Link>()
        val repository = SessionRepository(ApplicationProvider.getApplicationContext()) {
            Link(autoSnapshot = links.isEmpty()).also(links::add)
        }
        fun profile(room: String) = RelayProfile("wss://example.com", room, "a".repeat(43), "PC",
            "sha256/${"a".repeat(43)}=", "relay", SecureRelayProfile("b".repeat(43), "grant-$room", "c".repeat(43), false))
        try {
            val saved = profile("saved")
            val id = repository.connectRelay(saved)
            awaitState { repository.desktops.value.singleOrNull()?.status == "ready" }
            repository.setDraft("$id:1:2", "keep me")
            links.first().lost(java.io.IOException("offline"))
            awaitState { repository.desktops.value.single().status == "disconnected" }
            repository.connectRelay(profile("stalled"))
            awaitState { links[1].methods.contains("events.subscribe") }
            assertEquals(id, repository.connectRelay(profile("stalled")))
            assertEquals(2, links.size)
            assertEquals(id, repository.connectRelay(profile("replacement")))
            awaitState { links[2].methods.contains("events.subscribe") && links[1].closed }
            assertSame(saved, repository.relays.value.single())
            links[1].receive(JSONObject("""{"event":"runtime.snapshot","data":{"process_id":99,"windows":[]}}"""))
            links[1].lost(DesktopConnectionFailure(DesktopFailureKind.AUTHENTICATION))
            shadowOf(Looper.getMainLooper()).idle()
            assertEquals("connecting", repository.desktops.value.single().status)
            assertEquals(11L, repository.desktops.value.single().runtimeProcess)
            links[2].lost(DesktopConnectionFailure(DesktopFailureKind.AUTHENTICATION))
            awaitState { repository.desktops.value.single().status == "disconnected" }
            assertSame(saved, repository.relays.value.single())
            assertEquals("keep me", repository.drafts.value["$id:1:2"])
        } finally { repository.closeAll(); Dispatchers.resetMain() }
    }

    @Test fun lanDiscoveryKeepsTheComputerAndSavesTheAddressOnlyAfterAnAuthenticatedSnapshot() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val links = mutableListOf<Link>()
        val repository = SessionRepository(ApplicationProvider.getApplicationContext()) {
            Link(autoSnapshot = links.isEmpty()).also(links::add)
        }
        val pin = "sha256/${"a".repeat(43)}="
        val original = RelayProfile("wss://192.0.2.1:4000", "room", "a".repeat(43), "PC", pin, "lan",
            SecureRelayProfile("d".repeat(43), "device-grant", "c".repeat(43), false))
        val moved = PairingComputer("same-computer", "Untrusted advertised name", "https://192.0.2.2:4000".toHttpUrl(), pin)
        try {
            repository.foregroundChanged(true)
            val id = repository.connectRelay(original)
            awaitState { repository.desktops.value.singleOrNull()?.status == "ready" }
            repository.setDraft("$id:1:2", "unsent")
            links.first().lost(java.io.IOException("network changed"))
            awaitState { repository.desktops.value.single().status == "disconnected" }
            repository.recoverLanComputers(listOf(moved.copy(pin = "sha256/${"b".repeat(43)}=")))
            assertEquals(1, links.size)
            repository.recoverLanComputers(listOf(moved))
            awaitState { links.size == 2 && links[1].methods.contains("events.subscribe") }
            assertEquals(id, repository.desktops.value.single().id)
            assertSame(original, repository.relays.value.single())
            assertEquals("unsent", repository.drafts.value["$id:1:2"])

            links[1].receive(JSONObject("""{"event":"runtime.snapshot","data":{"process_id":11,"windows":[]}}"""))
            awaitState { repository.desktops.value.single().status == "ready" }
            val saved = repository.relays.value.single()
            assertEquals("wss://192.0.2.2:4000", saved.url)
            assertEquals(original.id, saved.id)
            assertEquals(original.name, saved.name)
            assertEquals(original.token, saved.token)
            assertSame(original.secure, saved.secure)
            assertTrue(links.all { link -> link.methods.none { it in setOf("pane.prompt", "pane.send_key") } })
            links.first().lost(DesktopConnectionFailure(DesktopFailureKind.AUTHENTICATION))
            shadowOf(Looper.getMainLooper()).idle()
            assertEquals("ready", repository.desktops.value.single().status)

            repository.foregroundChanged(false)
            links[1].lost(java.io.IOException("offline"))
            awaitState { repository.desktops.value.single().status == "disconnected" }
            repository.recoverLanComputers(listOf(moved.copy(address = "https://192.0.2.3:4000".toHttpUrl())))
            assertEquals(2, links.size)
            repository.closeDesktop(id)
            repository.foregroundChanged(true)
            repository.recoverLanComputers(listOf(moved))
            assertTrue(repository.desktops.value.isEmpty())
        } finally { repository.foregroundChanged(false); repository.closeAll(); Dispatchers.resetMain() }
    }
}
