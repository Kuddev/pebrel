package io.github.kuddev.pebrel.mobile.connection

import kotlinx.coroutines.*
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONObject
import java.io.Closeable
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

class DesktopRpcException(val code: String, message: String) : IllegalStateException(message)

/** One authority for bounded pending RPCs over SSH or a user-owned WSS relay. */
class DesktopRuntimeClient(
    private val transport: DesktopTransport,
    private val onSnapshot: (JSONObject) -> Unit,
    private val onDisconnected: () -> Unit,
) : Closeable {
    constructor(connection: SshConnection, onSnapshot: (JSONObject) -> Unit, onDisconnected: () -> Unit) :
        this(SshDesktopTransport(connection), onSnapshot, onDisconnected)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val writer = Mutex()
    private val sequence = AtomicLong()
    private val pending = ConcurrentHashMap<String, CompletableDeferred<JSONObject>>()
    private val ready = CompletableDeferred<JSONObject>()
    private val closed = AtomicBoolean()

    suspend fun connect(allowInput: Boolean): JSONObject {
        transport.open(allowInput, { message ->
            if (!closed.get()) {
                when {
                    message.optString("type") == "mobile.ready" -> ready.complete(message)
                    message.optString("type") == "mobile.disconnected" -> disconnect()
                    message.optString("event") == "runtime.snapshot" -> onSnapshot(message.getJSONObject("data"))
                    message.has("id") -> pending.remove(message.getString("id"))?.complete(message)
                }
            }
        }, ::disconnect)
        val hello = withTimeout(30_000) { ready.await() }
        check(hello.optString("protocol") in setOf("pebrel.mobile.ssh", "pebrel.mobile.relay") && hello.optInt("version") == 1)
        request("events.subscribe")
        return hello
    }

    suspend fun request(method: String, params: JSONObject = JSONObject()): JSONObject {
        val id = sequence.incrementAndGet().toString()
        val completion = CompletableDeferred<JSONObject>()
        val frame = JSONObject().put("id", id).put("method", method).put("params", params)
        require(frame.toString().toByteArray().size + 1 <= 40 * 1024)
        try {
            withContext(Dispatchers.IO) {
                writer.withLock {
                    check(!closed.get() && pending.size < 16)
                    pending[id] = completion
                    try {
                        transport.send(frame)
                    } catch (error: Exception) {
                        // A rejected send invalidates this connection as well as
                        // this request; settle all peers and leave the ready UI.
                        disconnect()
                        throw error
                    }
                }
            }
            val response = withTimeout(35_000) { completion.await() }
            if (!response.optBoolean("ok")) {
                val failure = response.optJSONObject("error")
                val code = failure?.optString("code")?.ifBlank { "runtime_error" } ?: "runtime_error"
                throw DesktopRpcException(code, failure?.optString("message")?.ifBlank { code } ?: code)
            }
            return response.optJSONObject("result") ?: JSONObject()
        } finally { pending.remove(id) }
    }
    private fun disconnect() {
        if (!closed.compareAndSet(false, true)) return
        settle()
        onDisconnected()
        scope.launch { try { transport.close() } finally { scope.cancel() } }
    }
    private fun settle() {
        ready.completeExceptionally(IllegalStateException("desktop_disconnected"))
        pending.values.forEach { it.completeExceptionally(IllegalStateException("delivery_unknown")) }
        pending.clear()
    }
    override fun close() {
        if (!closed.compareAndSet(false, true)) return
        settle()
        transport.close()
        scope.cancel()
    }
}

internal fun readBoundedFrame(input: java.io.InputStream, limit: Int): ByteArray? {
    val bytes = java.io.ByteArrayOutputStream(minOf(4096, limit))
    while (true) {
        val value = input.read()
        if (value < 0) {
            if (bytes.size() == 0) return null
            throw java.io.EOFException("incomplete_frame")
        }
        if (bytes.size() >= limit) throw java.io.IOException("frame_too_large")
        bytes.write(value)
        if (value == 10) return bytes.toByteArray()
    }
}
