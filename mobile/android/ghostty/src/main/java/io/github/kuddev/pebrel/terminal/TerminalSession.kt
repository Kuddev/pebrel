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
private class Input(private val reserved: Int, val encode: (GhosttyCore) -> ByteArray,
                    val prepareInput: Boolean, val wheel: Wheel?, val generation: Int) {
    val budget: Int get() = wheel?.let { kotlin.math.abs(it.lines) * 128 } ?: reserved
}
private class Wheel(var lines: Int, val column: Int, val row: Int, val generation: Int) {
    fun encode(engine: GhosttyCore): ByteArray {
        val output = java.io.ByteArrayOutputStream()
        var remaining = lines
        while (remaining != 0) {
            val step = remaining.coerceIn(-32, 32)
            output.write(engine.wheel(step, column, row, applicationOnly = true))
            remaining -= step
        }
        return output.toByteArray()
    }
}

/** Application-owned. I/O is bounded; parser/render state never runs on the main thread. */
class TerminalSession(private val transport: SessionTransport, private val callbacks: TerminalCallbacks,
                      private val scrollbackLines: Int = 1000) {
    private val main = Handler(Looper.getMainLooper())
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val stateDispatcher = Dispatchers.Default.limitedParallelism(1)
    private val closed = AtomicBoolean()
    private val started = AtomicBoolean()
    private val ended = AtomicBoolean()
    private val failed = AtomicBoolean()
    private val queuedBytes = AtomicInteger()
    private val acceptedInput = AtomicInteger()
    val inputGeneration: Int get() = acceptedInput.get()
    @Volatile var readingHistory: Boolean = false
        private set
    private val outgoing = Channel<Input>(64)
    private val inputLock = Any()
    private var pendingWheel: Wheel? = null
    private val sizes = Channel<Geometry>(Channel.CONFLATED)
    private val frames = Channel<Unit>(Channel.CONFLATED)
    private var core: GhosttyCore? = null
    private var synchronizedFrameTimeout: Job? = null
    private var synchronizedFrameExpired = false
    @Volatile private var geometry = Geometry()
    @Volatile private var palette: IntArray? = null
    @Volatile private var ready = false
    val isReady: Boolean get() = ready && !closed.get() && !failed.get()
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
                // The view is measured after the session is created. Give it a bounded
                // chance to supply its size before starting a full-screen application.
                val measured = withTimeoutOrNull(120) { sizes.receive() }
                val first = measured?.let { settledSize(it) } ?: geometry
                if (closed.get()) return@launch
                withContext(stateDispatcher) {
                    core = GhosttyCore(first.columns, first.rows, scrollbackLines).also { engine ->
                        engine.resize(first.columns, first.rows, first.width, first.height)
                        palette?.let(engine::colors)
                    }
                }
                transport.open(first.columns, first.rows, first.width, first.height)
                if (closed.get()) return@launch
                ready = true
                notify { callbacks.onTransportReady(this@TerminalSession) }
                sizes.trySend(geometry)
                requestFrame() // The first visible frame may have run before core initialization.
                launch { writeTransport() }
                launch { resizeTransport(first) }
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
                    withContext(stateDispatcher) {
                        cancelFrameTimeout()
                        core?.let { if (it.synchronizedOutput()) it.endSynchronizedOutput() }
                        requestFrame()
                    }
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
                    val engine = core ?: continue
                    // The parser applies synchronized output immediately; publication owns
                    // the last complete frame while a remote redraw replays its history.
                    if (engine.synchronizedOutput()) {
                        if (!synchronizedFrameExpired) {
                            if (synchronizedFrameTimeout == null) synchronizedFrameTimeout = scope.launch(stateDispatcher) {
                                delay(1000)
                                synchronizedFrameExpired = true
                                requestFrame() // A silent peer must not freeze the screen forever.
                            }
                            continue
                        }
                        engine.endSynchronizedOutput()
                    }
                    cancelFrameTimeout()
                    val next = engine.snapshot()
                    notify { frame = next; callbacks.onTextChanged(this@TerminalSession) }
                } catch (error: Exception) {
                    if (error is CancellationException) throw error
                    withContext(Dispatchers.IO) { fail(error) }
                }
            }
        }
    }

    private fun cancelFrameTimeout() {
        synchronizedFrameTimeout?.cancel()
        synchronizedFrameTimeout = null
        synchronizedFrameExpired = false
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
                synchronized(inputLock) { if (pendingWheel === input.wheel) pendingWheel = null }
                try {
                    if (closed.get()) break
                    val history = transport as? TerminalHistoryTransport
                    if (input.wheel != null && input.generation != inputGeneration) continue
                    // Native mouse reporting is the 2.2.1 fast path. Do not open SSH exec
                    // channels per gesture, and recheck the VT mode after queued output.
                    val wheelBytes = if (input.wheel != null) withContext(stateDispatcher) {
                        val engine = checkNotNull(core)
                        when (engine.applicationScrollMode()) {
                            0 -> byteArrayOf()
                            2 -> input.encode(engine)
                            else -> null
                        }
                    } else null
                    val consumed = try {
                        if (input.prepareInput) {
                            history?.returnToLiveInput()
                            readingHistory = false
                            requestFrame()
                        }
                        if (wheelBytes != null) {
                            if (wheelBytes.isNotEmpty()) history?.applicationScrolled()
                            true
                        } else input.wheel?.let { history?.scrollHistory(it.lines) } == true
                    } catch (error: Exception) {
                        if (error is CancellationException) throw error
                        reportRejected() // Do not send input into copy mode or close SSH on a history error.
                        continue
                    }
                    val bytes = wheelBytes ?: if (consumed) byteArrayOf() else withContext(stateDispatcher) { input.encode(checkNotNull(core)) }
                    if (bytes.isNotEmpty()) { transport.output().write(bytes); transport.output().flush() }
                } finally { queuedBytes.addAndGet(-input.budget) }
            }
        } catch (error: Exception) { if (!closed.get() && error !is CancellationException) fail(error) }
    }

    private suspend fun settledSize(first: Geometry): Geometry {
        var latest = first
        while (true) {
            latest = withTimeoutOrNull(120) { sizes.receiveCatching().getOrNull() } ?: return latest
        }
    }

    private suspend fun resizeTransport(initial: Geometry) {
        var applied = initial
        try {
            for (pending in sizes) {
                val size = settledSize(pending)
                if (closed.get()) break
                if (size == applied) continue
                // Commit one settled grid before notifying the peer. Intermediate IME
                // heights must not reset synchronized output or replay remote history.
                withContext(stateDispatcher) {
                    core?.resize(size.columns, size.rows, size.width, size.height)
                    requestFrame()
                }
                if (closed.get()) break
                transport.resize(size.columns, size.rows, size.width, size.height)
                applied = size
            }
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

    private fun offer(budget: Int, prepareInput: Boolean = false, wheel: Wheel? = null,
                      encode: (GhosttyCore) -> ByteArray): Boolean = synchronized(inputLock) {
        pendingWheel = null // Never merge gestures across text, keys or terminal replies.
        if (!ready || closed.get() || budget > 128 * 1024) return false
        val total = queuedBytes.addAndGet(budget)
        if (total > 128 * 1024 || !outgoing.trySend(Input(budget, encode, prepareInput, wheel, inputGeneration)).isSuccess) {
            queuedBytes.addAndGet(-budget)
            return false
        }
        pendingWheel = wheel
        return true
    }
    fun tryWrite(bytes: ByteArray, offset: Int, count: Int): Boolean {
        require(offset >= 0 && count >= 0 && offset <= bytes.size - count)
        if (count > 128 * 1024) return false
        val copy = bytes.copyOfRange(offset, offset + count)
        return offer(copy.size, prepareInput = true) { copy }.also { if (it) returnToInput() }
    }
    fun sendText(text: String): Boolean = text.toByteArray().let { tryWrite(it, 0, it.size) }
    fun key(code: Int, mods: Int = 0, action: Int = 1, text: String = "", unshifted: Int = 0): Boolean =
        offer(1024, prepareInput = true) { it.key(code, mods, action, text, unshifted) }.also { if (it && action != 0) returnToInput() }
    fun paste(text: String): Boolean {
        if (text.length > 32768) return false
        return offer(text.length * 4 + 12, prepareInput = true) { it.paste(text) }.also { if (it) returnToInput() }
    }
    /** Focusing the IME restores live input even before the first composed character is committed. */
    fun prepareForInput() {
        returnToInput()
        if (isReady && !offer(0, prepareInput = true) { byteArrayOf() }) reportRejected()
    }
    private fun returnToInput() {
        acceptedInput.incrementAndGet()
        scroll(Int.MAX_VALUE)
    }
    fun reportRejected() { notify { callbacks.onInputRejected(this@TerminalSession) } }
    private fun requestFrame() { if (visible) frames.trySend(Unit) }
    fun setVisible(value: Boolean) { visible = value; requestFrame() }
    fun updateSize(columns: Int, rows: Int, width: Int, height: Int) {
        val next = Geometry(columns.coerceIn(2, 400), rows.coerceIn(2, 200), width.coerceIn(1, 256), height.coerceIn(1, 256))
        if (next == geometry || closed.get()) return
        geometry = next
        sizes.trySend(next)
    }
    fun colors(value: IntArray) {
        palette = value.copyOf()
        scope.launch(stateDispatcher) { core?.colors(value); requestFrame() }
    }
    fun scroll(lines: Int) {
        scope.launch(stateDispatcher) { core?.scroll(lines); requestFrame() }
    }
    fun scrollTo(offset: Int) {
        scope.launch(stateDispatcher) { core?.scrollTo(offset); requestFrame() }
    }
    fun wheel(lines: Int, column: Int, row: Int) {
        if (lines == 0) return
        if (frame?.alternateScreen != true) { scroll(lines); return }
        readingHistory = true
        requestFrame()
        // Coalesce adjacent gestures while IO is busy, preserving direction, target and input order.
        val accepted = synchronized(inputLock) {
            val pending = pendingWheel
            val step = lines.coerceIn(-32, 32)
            if (pending != null && pending.generation == inputGeneration && pending.column == column && pending.row == row &&
                (pending.lines < 0) == (step < 0) && kotlin.math.abs(pending.lines + step) <= 256) {
                val extra = kotlin.math.abs(step) * 128
                if (queuedBytes.addAndGet(extra) <= 128 * 1024) {
                    pending.lines += step
                    true
                } else { queuedBytes.addAndGet(-extra); false }
            } else {
                val wheel = Wheel(step, column, row, inputGeneration)
                offer(kotlin.math.abs(step) * 128, wheel = wheel) { wheel.encode(it).also { requestFrame() } }
            }
        }
        if (!accepted) reportRejected()
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
