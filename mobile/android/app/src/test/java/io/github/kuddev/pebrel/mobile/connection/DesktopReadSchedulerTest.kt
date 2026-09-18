package io.github.kuddev.pebrel.mobile.connection

import kotlinx.coroutines.*
import kotlinx.coroutines.test.*
import org.junit.Assert.*
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class DesktopReadSchedulerTest {
    @Test fun inputWakesIdleImmediatelyWithoutConcurrentOrLostReads() = runTest {
        val reader = DesktopReadScheduler()
        var reads = 0
        var hold: CompletableDeferred<Unit>? = null
        val job = launch { reader.run { reads++; hold?.await(); false } }
        runCurrent()
        advanceTimeBy(2000)
        runCurrent()
        val before = reads
        hold = CompletableDeferred()
        reader.refresh()
        runCurrent()
        assertEquals(before + 1, reads) // No clock advance after the keystroke.
        repeat(1000) { reader.refresh() }
        runCurrent()
        assertEquals(before + 1, reads) // Only one request in flight.
        hold!!.complete(Unit)
        hold = null
        runCurrent()
        assertEquals(before + 2, reads) // One coalesced follow-up, never 1000.
        job.cancelAndJoin()
        advanceTimeBy(1000)
        assertEquals(before + 2, reads)
    }
}
