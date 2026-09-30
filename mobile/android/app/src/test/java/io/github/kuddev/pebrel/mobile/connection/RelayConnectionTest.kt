package io.github.kuddev.pebrel.mobile.connection

import kotlinx.coroutines.*
import okhttp3.*
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okhttp3.tls.HandshakeCertificates
import okhttp3.tls.HeldCertificate
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.util.concurrent.atomic.AtomicInteger

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class RelayConnectionTest {
    @Test fun discoveredAddressesNeverReplacePinnedIdentityOrInvitationCredentials() {
        val pin = "sha256/${"a".repeat(43)}="
        val secure = SecureRelayProfile("b".repeat(43), "device", "c".repeat(43), false)
        val saved = RelayProfile("wss://192.0.2.1:4000", "room", "a".repeat(43), "Saved name", pin, "lan", secure)
        val candidate = PairingComputer("anything", "Advertised name", "https://192.0.2.2:5000".toHttpUrl(), pin)
        val moved = checkNotNull(saved.discoveredAt(candidate))
        assertEquals(saved.id, moved.id)
        assertEquals(saved.device, moved.device)
        assertEquals(saved.token, moved.token)
        assertEquals(saved.name, moved.name)
        assertEquals(saved.tlsPin, moved.tlsPin)
        assertSame(secure, moved.secure)
        assertEquals("wss://192.0.2.2:5000", moved.url)
        assertTrue(moved.sameConnection(saved.atLanAddress("192.0.2.2:5000")))
        for (invalid in listOf("ws://192.0.2.2:5000", "user:pass@192.0.2.2:5000", "192.0.2.2:5000/path", "192.0.2.2:5000?token=other")) {
            assertThrows(IllegalArgumentException::class.java) { saved.atLanAddress(invalid) }
        }
        assertNull(saved.discoveredAt(candidate.copy(pin = "sha256/${"d".repeat(43)}=")))
        assertNull(saved.discoveredAt(candidate.copy(address = "https://192.0.2.2:5000/other".toHttpUrl())))
        assertNull(moved.discoveredAt(candidate))
        assertNull(RelayProfile(saved.url, saved.device, saved.token, saved.name, pin, "lan").discoveredAt(candidate))
        assertNull(RelayProfile(saved.url, saved.device, saved.token, saved.name, pin, "relay", secure).discoveredAt(candidate))
        val invite = SecureRelayProfile(secure.host, "invite", secure.secret, true, 9999999999)
        assertNull(RelayProfile(saved.url, saved.device, saved.token, saved.name, pin, "lan", invite).discoveredAt(candidate))
    }

    @Test fun aComputerIdentitySurvivesAddressRoomAndInvitationChanges() {
        fun secure(host: String, grant: String) = SecureRelayProfile(host, grant, "a".repeat(43), false)
        val first = RelayProfile("wss://192.0.2.1:4100", "first-room", "a".repeat(43), "PC",
            "sha256/${"a".repeat(43)}=", "lan", secure("b".repeat(43), "first-grant"))
        val next = RelayProfile("wss://192.0.2.2:4200", "next-room", "c".repeat(43), "Renamed PC",
            "sha256/${"d".repeat(43)}=", "lan", secure("b".repeat(43), "next-grant"))
        val another = RelayProfile(next.url, next.device, next.token, next.name, next.tlsPin, next.mode,
            secure("c".repeat(43), "next-grant"))
        assertEquals("routing changes must not create a second computer", first.id, next.id)
        assertNotEquals("same address and name do not prove the same computer", next.id, another.id)
        val legacy = RelayProfile(first.url, first.device, first.token, first.name)
        assertEquals(legacy.legacyId, legacy.id)
        assertNotEquals(legacy.id, RelayProfile(next.url, next.device, next.token, next.name).id)
    }

    @Test fun savedDuplicatesKeepTheLatestConnectionWithoutMergingOtherComputers() {
        fun key(value: Byte) = java.util.Base64.getUrlEncoder().withoutPadding().encodeToString(ByteArray(32) { value })
        fun profile(host: Byte, room: String) = RelayProfile.parse(RelayProfile("wss://example.com", room, key(1), "PC",
            "sha256/${"a".repeat(43)}=", "relay", SecureRelayProfile(key(host), "grant-$room", key(2), false)).toJson().toString())
        val old = profile(3, "old")
        val other = profile(4, "other")
        val latest = profile(3, "new")
        val restored = RelayProfile.latestByComputer(listOf(old, other, latest))
        assertEquals(listOf(other, latest), restored)
        assertNotEquals(old.legacyId, latest.legacyId)
        assertEquals(old.id, latest.id)
        assertEquals(restored, RelayProfile.latestByComputer(restored))
        assertTrue(latest.sameConnection(RelayProfile.parse(latest.toJson().toString())))
        assertFalse(old.sameConnection(latest))
    }

    @Test fun desktopTabLabelsAndLegacyBasenamesKeepTheOriginalPaths() {
        val path = "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"
        val pane = DesktopPane(1, 2, path, "C:\\project", "", "idle", 1)
        assertEquals("powershell.exe", pane.displayTitle)
        assertEquals(path, pane.title)
        assertEquals("work", pane.copy(tabLabel = "work").displayTitle)
        assertEquals("project", pane.copy(title = "/workspace/project/").displayTitle)
        assertEquals("project", pane.copy(title = "").displayTitle)
        val snapshot = JSONObject("""{"windows":[{"id":1,"tabs":[{"label":"work","panes":[{"id":2}]}]}]}""")
        snapshot.getJSONArray("windows").getJSONObject(0).getJSONArray("tabs").getJSONObject(0)
            .getJSONArray("panes").getJSONObject(0).put("title", path)
        val parsed = parseDesktopPanes(snapshot).single()
        assertEquals("work", parsed.displayTitle)
        assertEquals(path, parsed.title)
    }

    @Test fun interruptedTlsReadsRetryButHandshakeAndIdentityFailuresStayBlocked() {
        val interrupted = javax.net.ssl.SSLException("read interrupted")
        assertEquals(DesktopFailureKind.TLS, classifyDesktopFailure(interrupted))
        val network = classifyDesktopFailure(interrupted, tlsEstablished = true)
        assertEquals(DesktopFailureKind.NETWORK, network)
        assertTrue(DesktopReconnect.retryable(network))
        assertEquals(DesktopFailureKind.TLS, classifyDesktopFailure(
            javax.net.ssl.SSLHandshakeException("handshake failed"), tlsEstablished = true))
        assertEquals(DesktopFailureKind.CERTIFICATE_CHANGED, classifyDesktopFailure(
            javax.net.ssl.SSLException(PairingCertificateChanged()), tlsEstablished = true))
        assertEquals(DesktopFailureKind.CERTIFICATE_DATE, classifyDesktopFailure(
            javax.net.ssl.SSLException(java.security.cert.CertificateExpiredException()), tlsEstablished = true))
    }

    @Test fun aSnapshotCannotPublishASavedComputerBeforeTheProtocolIsValidated() = runBlocking {
        var published = 0
        val transport = object : DesktopTransport {
            override suspend fun open(allowInput: Boolean, receive: (JSONObject) -> Unit, disconnected: (Throwable?) -> Unit) {
                receive(JSONObject().put("event", "runtime.snapshot").put("data", JSONObject().put("process_id", 1)))
                receive(JSONObject().put("type", "mobile.ready").put("protocol", "wrong").put("version", 1))
            }
            override fun send(frame: JSONObject) = error("invalid protocol must not send")
            override fun close() = Unit
        }
        val client = DesktopRuntimeClient(transport, { published++ }, {})
        try {
            assertTrue(runCatching { client.connect(true) }.isFailure)
            assertEquals(0, published)
        } finally { client.close() }
    }

    @Test fun invitationsRequireTlsAndSeparateBoundedCredentials() {
        val value = JSONObject().put("version", 1).put("url", "wss://relay.example.com")
            .put("device", "computer").put("token", "a".repeat(43)).put("name", "PC")
        assertEquals("computer", RelayProfile.parse(value.toString()).device)
        for (url in listOf("ws://relay.example.com", "wss://user:pass@relay.example.com", "wss://relay.example.com?token=secret")) {
            assertThrows(IllegalArgumentException::class.java) { RelayProfile.parse(value.put("url", url).toString()) }
        }
    }

    @Test fun tlsWebSocketUsesHeaderCredentialsAndDisconnectSettlesInputWithoutReplay() = runBlocking {
        val certificate = HeldCertificate.Builder().commonName("localhost").addSubjectAlternativeName("localhost").build()
        val serverTls = HandshakeCertificates.Builder().heldCertificate(certificate).build()
        val server = MockWebServer()
        server.useHttps(serverTls.sslSocketFactory(), false)
        val sent = AtomicInteger()
        val disconnected = CompletableDeferred<Unit>()
        val snapshot = CompletableDeferred<JSONObject>()
        val epoch = "test-link"
        fun envelope(body: JSONObject) = JSONObject().put("type", "relay.data").put("link", epoch).put("body", body).toString()
        server.enqueue(MockResponse().withWebSocketUpgrade(object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) {
                webSocket.send(JSONObject().put("type", "relay.paired").put("link", epoch).toString())
                webSocket.send(envelope(JSONObject().put("type", "mobile.ready").put("protocol", "pebrel.mobile.relay")
                    .put("version", 1).put("capabilities", JSONObject().put("input", true))))
            }
            override fun onMessage(webSocket: WebSocket, text: String) {
                val frame = JSONObject(text)
                assertEquals(epoch, frame.getString("link"))
                val request = frame.getJSONObject("body")
                if (request.getString("method") == "events.subscribe") {
                    webSocket.send(envelope(JSONObject().put("id", request.getString("id")).put("ok", true).put("result", JSONObject())))
                    webSocket.send(envelope(JSONObject().put("event", "runtime.snapshot").put("data", JSONObject().put("process_id", 10))))
                } else {
                    sent.incrementAndGet()
                    webSocket.close(1012, "test_disconnect")
                }
            }
        }))
        server.start()
        val profile = RelayProfile.parse(JSONObject().put("version", 1).put("url", "wss://localhost:${server.port}")
            .put("device", "computer").put("token", "a".repeat(43)).put("mode", "lan")
            .put("tlsPin", CertificatePinner.pin(certificate.certificate)).toString())
        val http = PinnedDesktopTls.client(OkHttpClient(), requireNotNull(profile.tlsPin))
        val client = DesktopRuntimeClient(RelayTransport(profile, http), { snapshot.complete(it) }, { disconnected.complete(Unit) })
        try {
            withTimeout(10_000) {
                client.connect(true)
                assertEquals(10, snapshot.await().getInt("process_id"))
                val result = runCatching { client.request("pane.prompt", JSONObject().put("window_id", 1).put("pane_id", 2).put("text", "pwd")) }
                assertTrue(result.isFailure)
                disconnected.await()
                assertEquals(1, sent.get())
            }
            val upgrade = server.takeRequest()
            assertEquals("Bearer ${profile.token}", upgrade.getHeader("Authorization"))
            assertFalse(upgrade.path!!.contains(profile.token))
        } finally { client.close(); http.dispatcher.executorService.shutdown(); http.connectionPool.evictAll(); server.shutdown() }
    }

    @Test fun handshakeFailurePreservesTheCauseWithoutExposingRemoteText() = runBlocking {
        val expected = DesktopFailureKind.CERTIFICATE_CHANGED
        var observed: DesktopFailureKind? = null
        val transport = object : DesktopTransport {
            override suspend fun open(allowInput: Boolean, receive: (JSONObject) -> Unit, disconnected: (Throwable?) -> Unit) {
                disconnected(javax.net.ssl.SSLHandshakeException("private remote detail").apply {
                    initCause(PairingCertificateChanged())
                })
            }
            override fun send(frame: JSONObject) = error("No request may be sent before the handshake")
            override fun close() = Unit
        }
        val client = DesktopRuntimeClient(transport, {}, { observed = it })
        try {
            val failure = withTimeout(1000) { runCatching { client.connect(true) }.exceptionOrNull() }
            assertTrue(failure is DesktopConnectionFailure)
            assertEquals(expected, (failure as DesktopConnectionFailure).kind)
            assertEquals(expected, observed)
            assertEquals(expected.code, failure.message)
        } finally { client.close() }
    }

    @Test fun notificationReducerUsesDesktopFinishedStateAndRejectsDuplicateSequences() {
        fun snapshot(sequence: Int, state: String): JSONObject = JSONObject("""{
          "process_id":42,"windows":[{"id":1,"tabs":[{"label":"Test","panes":[
          {"id":2,"title":"Test","task_state":"$state","state_change_seq":$sequence}
          ]}]}]}""")
        val reducer = DesktopTransitions()
        assertTrue(reducer.observe(snapshot(1, "running")).isEmpty())
        assertEquals(1, reducer.observe(snapshot(2, "finished")).size)
        assertTrue(reducer.observe(snapshot(2, "finished")).isEmpty())
        assertTrue(reducer.observe(snapshot(1, "finished")).isEmpty())
        assertTrue(reducer.observe(snapshot(2, "finished")).isEmpty())
        assertTrue(reducer.observe(snapshot(3, "finished").put("mobile_policy", JSONObject().put("notifications", false))).isEmpty())
        assertTrue(reducer.observe(snapshot(3, "finished").put("mobile_policy", JSONObject().put("notifications", true))).isEmpty())
        assertEquals(1, reducer.observe(snapshot(4, "finished")).size)
    }

    @Test fun rejectedSendDisconnectsOnceAndSettlesOtherPendingRequests() = runBlocking {
        val pendingSent = CompletableDeferred<Unit>()
        val transportClosed = CompletableDeferred<Unit>()
        val disconnected = AtomicInteger()
        val sends = AtomicInteger()
        val closes = AtomicInteger()
        val transport = object : DesktopTransport {
            lateinit var receive: (JSONObject) -> Unit
            lateinit var lost: (Throwable?) -> Unit
            override suspend fun open(allowInput: Boolean, receive: (JSONObject) -> Unit, disconnected: (Throwable?) -> Unit) {
                this.receive = receive
                lost = disconnected
                receive(JSONObject().put("type", "mobile.ready").put("protocol", "pebrel.mobile.relay").put("version", 1))
            }
            override fun send(frame: JSONObject) {
                sends.incrementAndGet()
                when (frame.getString("method")) {
                    "events.subscribe" -> {
                        receive(JSONObject().put("id", frame.getString("id")).put("ok", true))
                        receive(JSONObject().put("event", "runtime.snapshot").put("data", JSONObject().put("process_id", 1)))
                    }
                    "pane.read" -> pendingSent.complete(Unit)
                    else -> throw java.io.IOException("send_rejected")
                }
            }
            override fun close() { closes.incrementAndGet(); lost(null); transportClosed.complete(Unit) }
        }
        val client = DesktopRuntimeClient(transport, {}, { disconnected.incrementAndGet() })
        try {
            withTimeout(5000) {
                client.connect(true)
                val pending = async { runCatching { client.request("pane.read") } }
                pendingSent.await()
                assertTrue(runCatching { client.request("pane.prompt") }.isFailure)
                assertTrue(pending.await().isFailure)
                transportClosed.await()
                assertTrue(runCatching { client.request("pane.prompt") }.isFailure)
                assertEquals(1, disconnected.get())
                assertEquals(1, closes.get())
                assertEquals(3, sends.get())
            }
        } finally { client.close() }
    }

    @Test fun shortCodeUsesPinnedTlsAndReturnsOnlyAnUnapprovedV2Invitation() = runBlocking {
        val certificate = HeldCertificate.Builder().commonName("localhost").addSubjectAlternativeName("localhost").build()
        val server = MockWebServer()
        server.useHttps(HandshakeCertificates.Builder().heldCertificate(certificate).build().sslSocketFactory(), false)
        server.start()
        val pin = CertificatePinner.pin(certificate.certificate)
        fun key(value: Int) = java.util.Base64.getUrlEncoder().withoutPadding().encodeToString(ByteArray(32) { value.toByte() })
        val invitation = RelayProfile("wss://localhost:${server.port}", "room", key(1), "Computer", pin, "lan",
            SecureRelayProfile(key(2), "invite", key(3), true, System.currentTimeMillis() / 1000 + 600))
        val observed = CompletableDeferred<String>()
        server.enqueue(MockResponse().withWebSocketUpgrade(object : WebSocketListener() {
            override fun onMessage(webSocket: WebSocket, text: String) {
                observed.complete(JSONObject(text).getString("code"))
                webSocket.send(JSONObject().put("invitation", invitation.toJson().toString()).toString())
                // 与真实短码端点一样，一次响应后主动完成关闭握手。
                webSocket.close(1000, null)
            }
        }))
        server.enqueue(MockResponse().withWebSocketUpgrade(object : WebSocketListener() {
            override fun onMessage(webSocket: WebSocket, text: String) {
                webSocket.send("{\"error\":\"pairing_code_invalid\"}")
                webSocket.close(1000, null)
            }
        }))
        // MockWebServer 会反查回环地址的主机名；本机 hosts 别名不应改变夹具证书的 SAN。
        val address = server.url("/").newBuilder().host("localhost").build()
        val computer = PairingComputer("fixture", "Computer", address, pin)
        val http = OkHttpClient()
        try {
            withTimeout(10_000) {
                val received = PairingCodeLookup.redeem(computer, "12345678", http)
                assertEquals(2, received.version)
                assertTrue(received.secure!!.invitation)
                assertEquals(invitation.id, received.id)
                assertEquals("12345678", observed.await())
                val request = server.takeRequest()
                assertEquals("/v2/pair", request.path)
                assertNull(request.getHeader("Authorization"))
                assertTrue(runCatching { PairingCodeLookup.redeem(computer, "87654321", http) }.exceptionOrNull() is PairingCodeRejected)
            }
        } finally { http.dispatcher.executorService.shutdown(); http.connectionPool.evictAll(); server.shutdown() }
    }

    @Test fun aLivePermissionChangeUpdatesAnIdleWorkspaceWithoutReconnecting() = runBlocking {
        val snapshots = mutableListOf<Boolean>()
        val transport = object : DesktopTransport {
            lateinit var receive: (JSONObject) -> Unit
            override suspend fun open(allowInput: Boolean, receive: (JSONObject) -> Unit, disconnected: (Throwable?) -> Unit) {
                this.receive = receive
                receive(JSONObject().put("type", "mobile.ready").put("protocol", "pebrel.mobile.relay").put("version", 1))
            }
            override fun send(frame: JSONObject) {
                receive(JSONObject().put("id", frame.getString("id")).put("ok", true))
                receive(JSONObject("""{"event":"runtime.snapshot","data":{"process_id":1,"windows":[],"mobile_policy":{"allow_input":true}}}"""))
            }
            override fun close() = Unit
        }
        val client = DesktopRuntimeClient(transport, { snapshots.add(it.getJSONObject("mobile_policy").getBoolean("allow_input")) }, {})
        try {
            client.connect(true)
            transport.receive(JSONObject("""{"type":"mobile.policy","allow_input":false}"""))
            transport.receive(JSONObject("""{"type":"mobile.policy","allow_input":true}"""))
            assertEquals(listOf(true, false, true), snapshots)
        } finally { client.close() }
    }
}
