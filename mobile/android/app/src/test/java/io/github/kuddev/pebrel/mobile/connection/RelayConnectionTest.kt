package io.github.kuddev.pebrel.mobile.connection

import kotlinx.coroutines.*
import okhttp3.*
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
}
