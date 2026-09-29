package io.github.kuddev.pebrel.mobile.connection

import kotlinx.coroutines.*
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withLock
import org.json.JSONObject

/** One visible, identity-bound screen subscription. Apply every delta in order;
 * acknowledge only after decode/publication, never when merely receiving bytes.
 */
internal class DesktopScreenStream(
    private val request: suspend (String, JSONObject) -> JSONObject,
    private val acknowledge: suspend (JSONObject) -> Deferred<Unit>,
    private val decoder: CoroutineDispatcher = Dispatchers.Default,
) {
    private class Desynchronized(cause: Throwable? = null) : java.io.IOException("screen_resync_required", cause)
    private val owner = Mutex()
    @Volatile private var inbox: Channel<JSONObject>? = null

    fun receive(frame: JSONObject) {
        if (frame.optString("event") == "pane.screen.heartbeat") return
        val target = inbox ?: return
        if (target.trySend(frame).isFailure) target.close(Desynchronized())
    }

    fun close(cause: Throwable) { inbox?.close(cause) }

    suspend fun run(params: JSONObject, consume: suspend (DesktopPaneRead) -> Unit) = owner.withLock {
        var recoveries = 0
        while (true) {
            try { subscribe(params, consume); break }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (error: Exception) {
                val recoverable = error is Desynchronized || error is DesktopRpcFailure && error.code == "screen_stream_lost"
                if (!recoverable || recoveries++ >= 2) throw error
                // Only rebuild read-only screen state, never reconnect or replay
                // uncertain input. Each replacement starts with a full snapshot.
                delay(100L * recoveries)
            }
        }
    }

    private suspend fun subscribe(params: JSONObject, consume: suspend (DesktopPaneRead) -> Unit) {
        val frames = Channel<JSONObject>(8)
        inbox = frames
        var subscription = 0L
        try {
            subscription = request("pane.screen.subscribe", params).getLong("subscription_id")
            require(subscription > 0)
            val identity = "${params.getLong("window_id")}:${params.getLong("pane_id")}"
            val sync = DesktopScreenSync()
            var sequence = 0L
            coroutineScope {
                val credit = Semaphore(4)
                for (frame in frames) {
                    if (frame.optLong("subscription_id") != subscription) continue
                    if (frame.optString("event") == "pane.screen.error") throw DesktopRpcFailure(
                        frame.optJSONObject("error")?.optString("code") ?: "screen_stream_failed")
                    val (next, read) = withContext(decoder) {
                        try {
                            val next = frame.getLong("sequence")
                            require(next == sequence + 1) { "screen_sequence_mismatch" }
                            val data = frame.getJSONObject("data")
                            require(data.getLong("window_id") == params.getLong("window_id") &&
                                data.getLong("pane_id") == params.getLong("pane_id"))
                            next to sync.apply(identity, data)
                        } catch (error: Exception) { throw Desynchronized(error) }
                    }
                    consume(read)
                    sequence = next
                    credit.acquire()
                    val receipt = try { acknowledge(control(params, subscription).put("sequence", next)) }
                    catch (error: Throwable) { credit.release(); throw error }
                    launch(start = CoroutineStart.UNDISPATCHED) {
                        try { receipt.await() }
                        finally { receipt.cancel(); credit.release() }
                    }
                }
            }
        } finally {
            if (inbox === frames) inbox = null
            frames.cancel()
            // Serializing replacement through owner prevents a late unsubscribe
            // from tearing down the next pane's stream. Never cancel/replay input.
            if (subscription > 0) withContext(NonCancellable) {
                withTimeoutOrNull(2000) {
                    try { request("pane.screen.unsubscribe", control(params, subscription)) }
                    catch (_: Exception) { }
                }
            }
        }
    }

    private fun control(params: JSONObject, subscription: Long) = JSONObject()
        .put("window_id", params.getLong("window_id")).put("pane_id", params.getLong("pane_id"))
        .put("subscription_id", subscription)
}
