package io.github.kuddev.pebrel.mobile.connection

import kotlinx.coroutines.*
import kotlinx.coroutines.channels.Channel

/** One read in flight. Input wakes an idle reader and is never lost during a read.
 * This remains a negotiated snapshot fallback, not a raw PTY subscription.
 */
internal class DesktopReadScheduler {
    private val wake = Channel<Unit>(Channel.CONFLATED)
    fun refresh() { wake.trySend(Unit) }

    suspend fun run(read: suspend () -> Boolean) {
        var interval = 0L
        while (currentCoroutineContext().isActive) {
            if (interval > 0 && withTimeoutOrNull(interval) { wake.receive(); true } == true) interval = 0
            val changed = read()
            interval = if (changed) 33 else (interval * 2).coerceIn(33, 500)
        }
    }
}
