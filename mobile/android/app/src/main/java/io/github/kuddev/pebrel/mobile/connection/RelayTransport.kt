package io.github.kuddev.pebrel.mobile.connection

import okhttp3.*
import okhttp3.HttpUrl.Companion.toHttpUrl
import okio.ByteString
import org.json.JSONObject
import java.security.MessageDigest
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

class RelayProfile(val url: String, val device: String, val token: String, val name: String) {
    val id: String = MessageDigest.getInstance("SHA-256").digest("$url/$device".toByteArray())
        .joinToString("") { "%02x".format(it) }
    fun toJson(): JSONObject = JSONObject().put("version", 1).put("url", url).put("device", device).put("token", token).put("name", name)
    override fun toString() = "RelayProfile($name)"
    companion object {
        fun parse(text: String): RelayProfile {
            require(text.length <= 8192)
            val data = JSONObject(text)
            require(data.getInt("version") == 1)
            val raw = data.getString("url")
            require(raw.startsWith("wss://"))
            val url = raw.replaceFirst("wss://", "https://").toHttpUrl()
            require(url.username.isEmpty() && url.password.isEmpty() && url.query == null && url.fragment == null && url.encodedPath == "/")
            val device = data.getString("device")
            val token = data.getString("token")
            require(Regex("[a-zA-Z0-9_-]{1,64}").matches(device))
            require(Regex("[a-zA-Z0-9_-]{43}").matches(token))
            val name = data.optString("name", device).take(80)
            return RelayProfile(url.toString().replaceFirst("https://", "wss://").trimEnd('/'), device, token, name)
        }
    }
}

/** TLS validates the user's server. Relay credentials never appear in a URL or log. */
class RelayTransport(
    private val profile: RelayProfile,
    private val client: OkHttpClient = sharedClient,
) : DesktopTransport {
    private val closed = AtomicBoolean()
    @Volatile private var link: String? = null
    @Volatile private var socket: WebSocket? = null
    override suspend fun open(allowInput: Boolean, receive: (JSONObject) -> Unit, disconnected: () -> Unit) {
        check(!closed.get())
        val url = profile.url.replaceFirst("wss://", "https://").toHttpUrl().newBuilder()
            .encodedPath("/v1/link").addQueryParameter("device", profile.device).addQueryParameter("role", "mobile").build()
        val request = Request.Builder().url(url).header("Authorization", "Bearer ${profile.token}").build()
        socket = client.newWebSocket(request, object : WebSocketListener() {
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
                        "relay.peer_left" -> { disconnected(); close() }
                        "relay.data" -> {
                            check(link != null && frame.getString("link") == link)
                            receive(frame.getJSONObject("body"))
                        }
                        else -> error("invalid_frame")
                    }
                } catch (_: Exception) { disconnected(); close() }
            }
            override fun onMessage(webSocket: WebSocket, bytes: ByteString) { disconnected(); close() }
            override fun onClosing(webSocket: WebSocket, code: Int, reason: String) { webSocket.close(code, null); if (!closed.get()) disconnected() }
            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) { if (!closed.get()) disconnected() }
            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) { if (!closed.get()) disconnected() }
        })
        if (closed.get()) socket?.cancel()
    }
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
            .readTimeout(0, TimeUnit.SECONDS).pingInterval(30, TimeUnit.SECONDS).retryOnConnectionFailure(false).build()
    }
}
