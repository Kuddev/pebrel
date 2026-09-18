package io.github.kuddev.pebrel.mobile.connection

import android.view.KeyEvent
import kotlinx.coroutines.*
import kotlinx.coroutines.test.*
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@OptIn(ExperimentalCoroutinesApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class DesktopTerminalInputTest {
    @Test fun interruptedAcknowledgementStopsPipelineAndSettlesDraftWithoutReplay() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val receipts = mutableListOf<CompletableDeferred<Unit>>()
        val rejected = mutableListOf<Boolean>()
        val input = DesktopTerminalInput({ _, _ -> error("sequential path") }, { true }, {}, { rejected += it },
            dispatch = { _, _ -> CompletableDeferred<Unit>().also(receipts::add) })
        try {
            val draft = async { input.submit("keep this draft") }
            runCurrent()
            assertEquals(2, receipts.size)
            receipts.first().cancel()
            runCurrent()
            assertFalse(draft.await())
            assertEquals(listOf(true), rejected)
            assertTrue(receipts.all { it.isCancelled })
            assertFalse(input.text("must not replay"))
        } finally { input.close(); Dispatchers.resetMain() }
    }

    @Test fun highRttInputPipelinesEightInOrderInsteadOfOneKeyPerRoundTrip() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val writes = mutableListOf<String>()
        val receipts = mutableListOf<CompletableDeferred<Unit>>()
        val failures = mutableListOf<Boolean>()
        val input = DesktopTerminalInput({ _, _ -> error("sequential path used") }, { true }, {}, { failures += it },
            dispatch = { _, params ->
                writes += params.getString("text")
                CompletableDeferred<Unit>().also(receipts::add)
            })
        try {
            repeat(12) { assertTrue(input.text(it.toString())) }
            runCurrent()
            assertEquals((0..7).map(Int::toString), writes)
            receipts.first().complete(Unit)
            runCurrent()
            assertEquals((0..8).map(Int::toString), writes)
            receipts[1].completeExceptionally(java.io.IOException("unknown delivery"))
            runCurrent()
            assertEquals(9, writes.size)
            assertEquals(listOf(true), failures)
            assertTrue(receipts.drop(2).all { it.isCancelled })
        } finally { input.close(); Dispatchers.resetMain() }
    }
    @Test fun printableTextDoesNotSubmitAndKeysKeepTheirOwnProtocol() {
        val command = DesktopTerminalInput.encodeText("你好 😀")!!.single()
        assertEquals("pane.prompt", command.method)
        assertFalse(command.params.getBoolean("submit"))
        val control = DesktopTerminalInput.encodeKey(KeyEvent.KEYCODE_C, 2, "c")!!.single()
        assertEquals("c", control.params.getString("key"))
        assertTrue(control.params.getJSONObject("modifiers").getBoolean("control"))
        assertNull(DesktopTerminalInput.encodeText("\u001b]52;clipboard"))
        assertNull(DesktopTerminalInput.encodeText("\n".repeat(129)))
        assertNull(DesktopTerminalInput.encodeKey(KeyEvent.KEYCODE_A, 8, "a"))
    }

    @Test fun orderedQueueStopsAfterUnknownDeliveryAndNeverReplays() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val writes = mutableListOf<String>()
        val failures = mutableListOf<Boolean>()
        val gate = CompletableDeferred<Unit>()
        val input = DesktopTerminalInput({ _, params ->
            writes += params.getString("text")
            gate.await()
            throw java.io.IOException("lost acknowledgement")
        }, { true }, {}, { failures += it })
        try {
            assertTrue(input.text("one"))
            assertTrue(input.text("two"))
            runCurrent()
            assertEquals(listOf("one"), writes)
            gate.complete(Unit)
            advanceUntilIdle()
            assertEquals(listOf("one"), writes)
            assertEquals(listOf(true), failures)
            assertFalse(input.text("three"))
        } finally { input.close(); Dispatchers.resetMain() }
    }

    @Test fun readOnlyStaleOwnersAndMultilineClipboardAreRejected() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        var active = true
        var count = 0
        val input = DesktopTerminalInput({ _, _ -> count++ }, { active }, {}, {})
        try {
            assertFalse(input.paste("rm example\n"))
            assertTrue(input.text("queued"))
            active = false
            advanceUntilIdle()
            assertEquals(0, count)
            assertFalse(input.text("read only"))
        } finally { input.close(); Dispatchers.resetMain() }
    }
}
