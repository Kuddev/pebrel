package io.github.kuddev.pebrel.terminal

import android.os.Handler
import android.os.Looper
import kotlinx.coroutines.*
import kotlinx.coroutines.channels.Channel
import java.io.IOException
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

open class TerminalCallbacks {
    open fun onTextChanged(session: TerminalSession) {}
    open fun onTitleChanged(session: TerminalSession) {}
    open fun onTransportReady(session: TerminalSession) {}
    open fun onSessionFinished(session: TerminalSession) {}
    open fun onInputRejected(session: TerminalSession) {}
}

private data class Geometry(val columns: Int = 80, val rows: Int = 24, val width: Int = 8, val height: Int = 16)
private class Input(val budget: Int, val encode: (GhosttyCore) -> ByteArray)

/** Application-owned. I/O is bounded; parser/render state never runs on the main thread. */
class TerminalSession(private val transport: SessionTransport, private val callbacks: TerminalCallbacks) {
    private val main = Handler(Looper.getMainLooper())
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val stateDispatcher = Dispatchers.Default.limitedParallelism(1)
    private val closed = AtomicBoolean()
    private val started = AtomicBoolean()
    private val ended = AtomicBoolean()
    private val failed = AtomicBoolean()
    private val queuedBytes = AtomicInteger()
    private val outgoing = Channel<Input>(64)
    private val sizes = Channel<Geometry>(Channel.CONFLATED)
    private val frames = Channel<Unit>(Channel.CONFLATED)
    private var core: GhosttyCore? = null
    @Volatile private var geometry = Geometry()
    @Volatile private var palette: IntArray? = null
    @Volatile private var ready = false
    @Volatile private var visible = false
    @Volatile var frame: TerminalFrame? = null
        private set
    @Volatile var title: String? = null
        private set
    @Volatile var failure: String? = null
        private set
    @Volatile var failureCause: Exception? = null
        private set

    fun start() {
        if (!started.compareAndSet(false, true) || closed.get()) return
        scope.launch {
            try {
                withContext(stateDispatcher) {
                    val size = geometry
                    core = GhosttyCore(size.columns, size.rows).also { engine ->
                        engine.resize(size.columns, size.rows, size.width, size.height)
                        palette?.let(engine::colors)
                    }
                }
                val first = geometry
                transport.open(first.columns, first.rows, first.width, first.height)
                if (closed.get()) return@launch
                ready = true
                notify { callbacks.onTransportReady(this@TerminalSession) }
                sizes.trySend(geometry)
                launch { writeTransport() }
                launch { resizeTransport() }
                readTransport()
            } catch (error: Exception) {
                if (!closed.get() && error !is CancellationException) fail(error)
            } catch (error: LinkageError) {
                if (!closed.get()) fail(IllegalStateException("terminal_engine_unavailable", error))
            } finally {
                ready = false
                outgoing.close()
                sizes.close()
                withContext(NonCancellable) {
                    transport.close()
                    if (transport is LocalPtyTransport) runCatching { transport.awaitExit() }
                }
                complete()
            }
        }
        scope.launch(stateDispatcher) {
            for (ignored in frames) {
                delay(16) // Coalesce bursts. No timer runs when output is unchanged or the view is detached.
                if (!visible || closed.get()) continue
                try {
                    val next = core?.snapshot() ?: continue
                    notify { frame = next; callbacks.onTextChanged(this@TerminalSession) }
                } catch (error: Exception) {
                    if (error is CancellationException) throw error
                    withContext(Dispatchers.IO) { fail(error) }
                }
            }
        }
    }

    private suspend fun readTransport() {
        val buffer = ByteArray(16384)
        while (!closed.get()) {
            val count = try { transport.input().read(buffer) } catch (error: IOException) {
                // Linux PTY reports EIO at EOF; its waitpid result owns process completion.
                if (transport is LocalPtyTransport) break else throw error
            }
            if (count < 0) break
            if (count == 0) continue
            withContext(stateDispatcher) {
                val engine = checkNotNull(core)
                val response = engine.feed(buffer, count)
                if (response.isNotEmpty() && !offer(response.size) { response }) throw IOException("terminal_reply_rejected")
                engine.takeTitle()?.let { changed ->
                    title = changed
                    notify { callbacks.onTitleChanged(this@TerminalSession) }
                }
                requestFrame()
            }
        }
        transport.awaitExit()
    }

    private suspend fun writeTransport() {
        try {
            for (input in outgoing) {
                try {
                    if (closed.get()) break
                    val bytes = withContext(stateDispatcher) { input.encode(checkNotNull(core)) }
                    if (bytes.isNotEmpty()) { transport.output().write(bytes); transport.output().flush() }
                } finally { queuedBytes.addAndGet(-input.budget) }
            }
        } catch (error: Exception) { if (!closed.get() && error !is CancellationException) fail(error) }
    }

    private suspend fun resizeTransport() {
        try {
            for (size in sizes) if (!closed.get()) transport.resize(size.columns, size.rows, size.width, size.height)
        } catch (error: Exception) { if (!closed.get() && error !is CancellationException) fail(error) }
    }

    private fun fail(error: Exception) {
        if (!failed.compareAndSet(false, true)) return
        failureCause = error
        failure = error.javaClass.simpleName
        ready = false
        // Feedback must not wait for a peer's potentially slow channel shutdown.
        complete()
        transport.close() // Already on IO dispatcher; unblocks the peer reader/writer.
    }
    private fun complete() {
        if (ended.compareAndSet(false, true)) notify { callbacks.onSessionFinished(this@TerminalSession) }
    }
    private fun notify(action: () -> Unit) { main.post { if (!closed.get()) action() } }

    private fun offer(budget: Int, encode: (GhosttyCore) -> ByteArray): Boolean {
        if (!ready || closed.get() || budget > 128 * 1024) return false
        val total = queuedBytes.addAndGet(budget)
        if (total > 128 * 1024 || !outgoing.trySend(Input(budget, encode)).isSuccess) {
            queuedBytes.addAndGet(-budget)
            return false
        }
        return true
    }
    fun tryWrite(bytes: ByteArray, offset: Int, count: Int): Boolean {
        require(offset >= 0 && count >= 0 && offset <= bytes.size - count)
        if (count > 128 * 1024) return false
        val copy = bytes.copyOfRange(offset, offset + count)
        return offer(copy.size) { copy }.also { if (it) scroll(Int.MAX_VALUE) }
    }
    fun sendText(text: String): Boolean = text.toByteArray().let { tryWrite(it, 0, it.size) }
    fun key(code: Int, mods: Int = 0, action: Int = 1, text: String = "", unshifted: Int = 0): Boolean =
        offer(1024) { it.key(code, mods, action, text, unshifted) }.also { if (it && action != 0) scroll(Int.MAX_VALUE) }
    fun paste(text: String): Boolean {
        if (text.length > 32768) return false
        return offer(text.length * 4 + 12) { it.paste(text) }.also { if (it) scroll(Int.MAX_VALUE) }
    }
    fun reportRejected() { notify { callbacks.onInputRejected(this@TerminalSession) } }
    private fun requestFrame() { if (visible) frames.trySend(Unit) }
    fun setVisible(value: Boolean) { visible = value; requestFrame() }
    fun updateSize(columns: Int, rows: Int, width: Int, height: Int) {
        val next = Geometry(columns.coerceIn(2, 400), rows.coerceIn(2, 200), width.coerceIn(1, 256), height.coerceIn(1, 256))
        if (next == geometry || closed.get()) return
        geometry = next
        scope.launch(stateDispatcher) {
            core?.resize(next.columns, next.rows, next.width, next.height)
            requestFrame()
        }
        if (ready) sizes.trySend(next)
    }
    fun colors(value: IntArray) {
        palette = value.copyOf()
        scope.launch(stateDispatcher) { core?.colors(value); requestFrame() }
    }
    fun scroll(lines: Int) {
        scope.launch(stateDispatcher) { core?.scroll(lines); requestFrame() }
    }
    fun previewText(): String {
        val current = frame ?: return ""
        val end = current.cursorY.coerceIn(0, current.rows.lastIndex)
        return current.rows.slice((end - 8).coerceAtLeast(0)..end)
            .joinToString("\n") { it?.text?.trimEnd().orEmpty() }.take(2048)
    }
    fun finishIfRunning() {
        if (!closed.compareAndSet(false, true)) return
        ready = false
        scope.cancel()
        outgoing.cancel()
        sizes.cancel()
        frames.cancel()
        scope.launch(NonCancellable) {
            transport.close()
            withContext(stateDispatcher) { core?.close(); core = null; frame = null }
        }
    }
}
