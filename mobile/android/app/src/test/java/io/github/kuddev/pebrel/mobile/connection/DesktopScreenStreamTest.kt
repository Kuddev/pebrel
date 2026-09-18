package io.github.kuddev.pebrel.mobile.connection

import kotlinx.coroutines.*
import kotlinx.coroutines.test.*
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@OptIn(ExperimentalCoroutinesApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class DesktopScreenStreamTest {
    private fun params(pane: Long = 2) = JSONObject().put("window_id", 1).put("pane_id", pane).put("lines", 100)
    private fun event(id: Long, seq: Long, pane: Long = 2) = JSONObject().put("event", "pane.screen")
        .put("subscription_id", id).put("sequence", seq).put("data", params(pane).put("screen_seq", seq)
            .put("screen", JSONObject().put("version", 1).put("columns", 1)
                .put("rows", JSONArray("[[[\"x\",1,0,0,0]]]")).put("cursor", JSONArray("[0,0,1]"))
                .put("palette", JSONArray())))

    @Test fun outputPushesWithoutPollingAndDoesNotWaitForAckRoundTrip() = runTest {
        val methods = mutableListOf<String>()
        val observed = mutableListOf<Long>()
        val ack = mutableListOf<CompletableDeferred<Unit>>()
        val stream = DesktopScreenStream({ method, _ -> methods += method; JSONObject().put("subscription_id", 7) }, {
            assertEquals(observed.last(), it.getLong("sequence")) // Publish before ACK.
            CompletableDeferred<Unit>().also(ack::add)
        }, StandardTestDispatcher(testScheduler))
        val job = launch { stream.run(params()) { observed += it.response.getLong("screen_seq") } }
        runCurrent()
        repeat(4) { stream.receive(event(7, it + 1L)); runCurrent() }
        assertEquals(listOf(1L, 2L, 3L, 4L), observed)
        assertEquals(4, ack.size)
        assertEquals(listOf("pane.screen.subscribe"), methods)
        job.cancelAndJoin()
        assertTrue(ack.all { it.isCancelled })
        assertEquals(listOf("pane.screen.subscribe", "pane.screen.unsubscribe"), methods)
    }

    @Test fun replacementRejectsOldEventsAndSequenceGapResubscribesToFullSnapshot() = runTest {
        var id = 4L
        val observed = mutableListOf<Long>()
        val methods = mutableListOf<String>()
        val stream = DesktopScreenStream({ method, _ ->
            methods += method
            JSONObject().put("subscription_id", id)
        }, { CompletableDeferred(Unit) }, StandardTestDispatcher(testScheduler))
        val first = launch { stream.run(params()) { observed += it.response.getLong("screen_seq") } }
        runCurrent()
        first.cancelAndJoin()
        id = 5
        val second = async { runCatching { stream.run(params(3)) { observed += it.response.getLong("screen_seq") } } }
        runCurrent()
        stream.receive(event(4, 1)) // Retired subscription must not poison the new pane.
        stream.receive(event(5, 1, 3))
        runCurrent()
        assertEquals(listOf(1L), observed)
        stream.receive(event(5, 3, 3))
        runCurrent()
        assertEquals(listOf(1L), observed)
        id = 6
        advanceTimeBy(100); runCurrent()
        stream.receive(event(5, 4, 3))
        stream.receive(event(6, 1, 3))
        runCurrent()
        assertEquals(listOf(1L, 1L), observed)
        assertEquals(3, methods.count { it == "pane.screen.subscribe" })
        assertFalse(methods.contains("pane.prompt"))
        second.cancelAndJoin()
    }

    @Test fun malformedStreamsStopAfterTwoReadOnlyRecoveries() = runTest {
        var subscriptions = 0L
        val stream = DesktopScreenStream({ method, _ ->
            if (method == "pane.screen.subscribe") subscriptions++
            JSONObject().put("subscription_id", subscriptions)
        }, { CompletableDeferred(Unit) }, StandardTestDispatcher(testScheduler))
        val job = async { runCatching { stream.run(params()) { error("invalid frame published") } } }
        repeat(3) {
            runCurrent()
            stream.receive(event(subscriptions, 3))
            runCurrent()
            advanceTimeBy(300)
        }
        runCurrent()
        assertTrue(job.await().isFailure)
        assertEquals(3L, subscriptions)
    }
}
