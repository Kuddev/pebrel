package io.github.kuddev.pebrel.mobile.connection

import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withTimeout
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.ByteString
import org.json.JSONObject
import java.io.IOException
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

internal class PairingCodeRejected : IOException("pairing_code_invalid")

/** The short code retrieves an invitation, not a Runtime session or a device grant. */
internal object PairingCodeLookup {
    private val client = OkHttpClient.Builder().connectTimeout(5, TimeUnit.SECONDS)
        .readTimeout(5, TimeUnit.SECONDS).followRedirects(false).followSslRedirects(false)
        .retryOnConnectionFailure(false).build()

    suspend fun redeem(computer: PairingComputer, code: String, base: OkHttpClient = client): RelayProfile {
        require(Regex("[0-9]{8}").matches(code))
        val pinned = PinnedDesktopTls.client(base, computer.pin)
        val request = Request.Builder().url(computer.address.newBuilder().encodedPath("/v2/pair").build()).build()
        try {
            return withTimeout(12_000) {
                suspendCancellableCoroutine { continuation ->
                    val finished = AtomicBoolean()
                    val socket = AtomicReference<WebSocket?>()
                    fun fail(error: Throwable) {
                        if (finished.compareAndSet(false, true)) {
                            socket.get()?.cancel()
                            continuation.resumeWithException(error)
                        }
                    }
                    continuation.invokeOnCancellation { finished.set(true); socket.get()?.cancel() }
                    val connection = pinned.newWebSocket(request, object : WebSocketListener() {
                        override fun onOpen(webSocket: WebSocket, response: Response) {
                            if (finished.get()) { webSocket.cancel(); return }
                            if (!webSocket.send(JSONObject().put("code", code).toString())) fail(IOException("pairing_send_failed"))
                        }
                        override fun onMessage(webSocket: WebSocket, text: String) {
                            if (finished.get()) return
                            try {
                                require(text.length <= 16_384 && text.toByteArray(Charsets.UTF_8).size <= 16_384)
                                val response = JSONObject(text)
                                if (response.has("error")) throw PairingCodeRejected()
                                val profile = RelayProfile.parse(response.getString("invitation"))
                                val expectedUrl = computer.address.toString().replaceFirst("https://", "wss://").trimEnd('/')
                                require(profile.version == 2 && profile.mode == "lan" && profile.tlsPin == computer.pin && profile.url == expectedUrl)
                                require(profile.secure?.invitation == true && profile.secure!!.expiresAt > System.currentTimeMillis() / 1000)
                                // 广播指纹只用于这次引导；最终身份仍由两端本地 Noise 校验码和桌面批准确认。
                                if (finished.compareAndSet(false, true)) {
                                    webSocket.close(1000, null)
                                    continuation.resume(profile)
                                }
                            } catch (error: Exception) { fail(error) }
                        }
                        override fun onMessage(webSocket: WebSocket, bytes: ByteString) = fail(IOException("invalid_pairing_response"))
                        override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
                            webSocket.close(code, null)
                            fail(IOException("pairing_closed"))
                        }
                        override fun onFailure(webSocket: WebSocket, error: Throwable, response: Response?) = fail(error)
                    })
                    socket.set(connection)
                    if (finished.get()) connection.cancel()
                }
            }
        } catch (error: TimeoutCancellationException) {
            throw IOException("pairing_timeout", error)
        }
    }
}
