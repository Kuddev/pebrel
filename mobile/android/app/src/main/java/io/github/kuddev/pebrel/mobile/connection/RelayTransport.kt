package io.github.kuddev.pebrel.mobile.connection

import okhttp3.*
import okhttp3.HttpUrl.Companion.toHttpUrl
import okio.ByteString
import org.json.JSONObject
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/** TLS validates the user's server. Relay credentials never appear in a URL or log. */
class RelayTransport(
    private val profile: RelayProfile,
    private val client: OkHttpClient = profile.tlsPin?.let { PinnedDesktopTls.client(sharedClient, it) } ?: sharedClient,
) : DesktopTransport {
    private val closed = AtomicBoolean()
    @Volatile private var link: String? = null
    @Volatile private var socket: WebSocket? = null
    @Volatile private var relayReached = false
    override suspend fun open(allowInput: Boolean, receive: (JSONObject) -> Unit, disconnected: (Throwable?) -> Unit) {
        check(!closed.get())
        val url = profile.url.replaceFirst("wss://", "https://").toHttpUrl().newBuilder()
            .encodedPath("/v1/link").addQueryParameter("device", profile.device).addQueryParameter("role", "mobile").build()
        val request = Request.Builder().url(url).header("Authorization", "Bearer ${profile.token}").build()
        fun fail(kind: DesktopFailureKind, cause: Throwable? = null) {
            if (!closed.compareAndSet(false, true)) return
            link = null
            disconnected(DesktopConnectionFailure(kind, cause))
            socket?.cancel()
        }
        socket = client.newWebSocket(request, object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) { relayReached = true }
            override fun onMessage(webSocket: WebSocket, text: String) {
                if (closed.get()) return
                try {
                    if (text.length > MAX_FRAME || text.toByteArray().size > MAX_FRAME) error("frame_too_large")
                    val frame = JSONObject(text)
                    when (frame.getString("type")) {
                        "relay.waiting" -> Unit
                        "relay.paired" -> {
                            check(link == null)
                            val newLink = frame.getString("link")
                            require(newLink.length in 1..80)
                            link = newLink
                        }
                        "relay.peer_left" -> fail(DesktopFailureKind.PEER_OFFLINE)
                        "relay.data" -> {
                            check(link != null && frame.getString("link") == link)
                            receive(frame.getJSONObject("body"))
                        }
                        else -> error("invalid_frame")
                    }
                } catch (error: Exception) { fail(DesktopFailureKind.PROTOCOL, error) }
            }
            override fun onMessage(webSocket: WebSocket, bytes: ByteString) { fail(DesktopFailureKind.PROTOCOL) }
            override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
                webSocket.close(code, null)
                fail(if (code == 1008 || code == 1009) DesktopFailureKind.PROTOCOL else DesktopFailureKind.DISCONNECTED)
            }
            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) { fail(DesktopFailureKind.DISCONNECTED) }
            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                fail(desktopHttpFailure(response?.code) ?: classifyDesktopFailure(t), t)
            }
        })
        if (closed.get()) socket?.cancel()
    }
    override fun readyTimeoutFailure(): DesktopFailureKind =
        if (relayReached) DesktopFailureKind.PEER_OFFLINE else DesktopFailureKind.TIMEOUT

    override fun send(frame: JSONObject) {
        check(!closed.get())
        val socket = checkNotNull(socket)
        val epoch = checkNotNull(link)
        check(socket.queueSize() <= MAX_FRAME)
        val envelope = JSONObject().put("type", "relay.data").put("link", epoch).put("body", frame).toString()
        check(envelope.toByteArray().size <= MAX_FRAME && socket.send(envelope))
    }
    override fun close() {
        if (!closed.compareAndSet(false, true)) return
        link = null
        socket?.cancel()
    }
    companion object {
        private const val MAX_FRAME = 2 * 1024 * 1024 + 1024
        private val sharedClient = OkHttpClient.Builder().connectTimeout(15, TimeUnit.SECONDS)
            .readTimeout(0, TimeUnit.SECONDS).pingInterval(30, TimeUnit.SECONDS)
            .followRedirects(false).followSslRedirects(false).retryOnConnectionFailure(false).build()
    }
}
