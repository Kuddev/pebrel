package io.github.kuddev.pebrel.mobile.connection

import kotlinx.coroutines.*
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONObject
import java.io.Closeable
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

internal class DesktopRpcFailure(val code: String) : java.io.IOException(code)

/** One authority for bounded pending RPCs over SSH or a user-owned WSS relay. */
class DesktopRuntimeClient(
    private val transport: DesktopTransport,
    private val onSnapshot: (JSONObject) -> Unit,
    private val onDisconnected: (DesktopFailureKind) -> Unit,
) : Closeable {
    constructor(connection: SshConnection, onSnapshot: (JSONObject) -> Unit, onDisconnected: (DesktopFailureKind) -> Unit) :
        this(SshDesktopTransport(connection), onSnapshot, onDisconnected)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val writer = Mutex()
    private val sequence = AtomicLong()
    private val pending = ConcurrentHashMap<String, CompletableDeferred<JSONObject>>()
    private val ready = CompletableDeferred<JSONObject>()
    private val firstSnapshot = CompletableDeferred<Unit>()
    private val snapshotLock = Any()
    private var latestSnapshot: JSONObject? = null
    private var snapshotsActive = false
    private val closed = AtomicBoolean()
    @Volatile private var screenUnsupported = false
    @Volatile private var screenDelta = false
    @Volatile private var screenStream = false
    private val streaming = DesktopScreenStream(::request, acknowledge = { params ->
        val call = startRequest("pane.screen.ack", params)
        scope.async(start = CoroutineStart.UNDISPATCHED) { finishRequest(call); Unit }
    })
    suspend fun streamPane(params: JSONObject, consume: suspend (DesktopPaneRead) -> Unit): Boolean {
        if (!screenStream) return false
        streaming.run(params, consume)
        return true
    }
    private val screenSync = DesktopScreenSync()
    private val screenReader = Mutex()
    suspend fun resetScreen() = screenReader.withLock { screenSync.reset() }

    suspend fun readPane(params: JSONObject): DesktopPaneRead = screenReader.withLock {
        if (screenUnsupported) return@withLock DesktopPaneRead(request("pane.read", params))
        val identity = "${params.getLong("window_id")}:${params.getLong("pane_id")}"
        val query = JSONObject(params.toString()).put("screen", true)
        if (screenDelta) query.put("screen_since", screenSync.since(identity))
        try {
            val response = request("pane.read", query)
            if (!screenDelta) return@withLock DesktopPaneRead(response)
            try { screenSync.apply(identity, response) }
            catch (_: Exception) {
                // Only this idempotent read can be repeated; never resend input.
                screenSync.reset()
                screenSync.apply(identity, request("pane.read", query.put("screen_since", 0)))
            }
        } catch (failure: DesktopRpcFailure) {
            // Old desktop runtimes reject unknown parameters. Do not silently
            // downgrade malformed snapshots, transport errors or authorization.
            if (failure.code != "invalid_params") throw failure
            screenUnsupported = true
            DesktopPaneRead(request("pane.read", params))
        }
    }

    suspend fun connect(allowInput: Boolean): JSONObject {
        try {
            transport.open(allowInput, { message ->
                if (!closed.get()) {
                    when {
                        message.optString("type") == "mobile.ready" -> ready.complete(message)
                        message.optString("type") == "mobile.disconnected" -> disconnect(
                            DesktopConnectionFailure(DesktopFailureKind.RUNTIME_UNAVAILABLE))
                        message.optString("event").startsWith("pane.screen") -> streaming.receive(message)
                        message.optString("event") == "runtime.snapshot" -> synchronized(snapshotLock) {
                            val snapshot = message.getJSONObject("data")
                            latestSnapshot = snapshot
                            firstSnapshot.complete(Unit)
                            if (snapshotsActive && !closed.get()) onSnapshot(snapshot)
                        }
                        message.has("id") -> pending.remove(message.getString("id"))?.complete(message)
                    }
                }
            }, ::disconnect)
            val hello = try { withTimeout(30_000) { ready.await() } }
            catch (error: TimeoutCancellationException) {
                throw DesktopConnectionFailure(transport.readyTimeoutFailure(), error)
            }
            if (hello.optString("protocol") !in setOf("pebrel.mobile.ssh", "pebrel.mobile.relay") || hello.optInt("version") != 1) {
                throw DesktopConnectionFailure(DesktopFailureKind.PROTOCOL)
            }
            screenDelta = hello.optJSONObject("capabilities")?.optBoolean("screen_delta") == true
            screenStream = hello.optJSONObject("capabilities")?.optBoolean("terminal_grid_stream") == true
            try { request("events.subscribe") }
            catch (error: CancellationException) { throw error }
            catch (error: Exception) {
                throw if (error is DesktopConnectionFailure) error
                    else DesktopConnectionFailure(DesktopFailureKind.RUNTIME_UNAVAILABLE, error)
            }
            withTimeout(30_000) { firstSnapshot.await() }
            synchronized(snapshotLock) {
                check(!closed.get())
                snapshotsActive = true
                onSnapshot(checkNotNull(latestSnapshot))
            }
            return hello
        } catch (error: TimeoutCancellationException) {
            val failure = DesktopConnectionFailure(DesktopFailureKind.RUNTIME_UNAVAILABLE, error)
            disconnect(failure)
            throw failure
        } catch (cancelled: CancellationException) {
            close()
            throw cancelled
        } catch (error: Exception) {
            val failure = if (error is DesktopConnectionFailure) error else DesktopConnectionFailure(classifyDesktopFailure(error), error)
            disconnect(failure)
            throw failure
        }
    }

    suspend fun request(method: String, params: JSONObject = JSONObject()): JSONObject {
        return finishRequest(startRequest(method, params))
    }

    /** Send in wire order, without spending a network RTT before the next key.
     * The input owner bounds this pipeline and must cancel abandoned receipts.
     */
    internal suspend fun dispatchInput(method: String, params: JSONObject): Deferred<Unit> {
        require(method == "pane.prompt" || method == "pane.send_key")
        val call = startRequest(method, params)
        return scope.async(start = CoroutineStart.UNDISPATCHED) { finishRequest(call); Unit }
    }

    private data class Call(val id: String, val completion: CompletableDeferred<JSONObject>)

    private suspend fun startRequest(method: String, params: JSONObject): Call {
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
                        disconnect(error)
                        throw error
                    }
                }
            }
            return Call(id, completion)
        } catch (error: Throwable) {
            pending.remove(id)
            throw error
        }
    }

    private suspend fun finishRequest(call: Call): JSONObject {
        try {
            val response = withTimeout(35_000) { call.completion.await() }
            if (!response.optBoolean("ok")) throw DesktopRpcFailure(
                response.optJSONObject("error")?.optString("code")?.take(80) ?: "runtime_error")
            return response.optJSONObject("result") ?: JSONObject()
        } catch (timeout: TimeoutCancellationException) {
            val failure = DesktopConnectionFailure(DesktopFailureKind.TIMEOUT, timeout)
            disconnect(failure)
            throw failure
        } finally { pending.remove(call.id) }
    }
    private fun disconnect(error: Throwable?) {
        if (!closed.compareAndSet(false, true)) return
        val failure = DesktopConnectionFailure(if (error == null) DesktopFailureKind.DISCONNECTED else classifyDesktopFailure(error), error)
        settle(failure)
        onDisconnected(failure.kind)
        scope.launch { try { transport.close() } finally { scope.cancel() } }
    }
    private fun settle(failure: DesktopConnectionFailure = DesktopConnectionFailure(DesktopFailureKind.DISCONNECTED)) {
        streaming.close(failure)
        ready.completeExceptionally(failure)
        firstSnapshot.completeExceptionally(failure)
        pending.values.forEach { it.completeExceptionally(failure) }
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
