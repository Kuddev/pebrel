package io.github.kuddev.pebrel.mobile.connection

import android.app.Application
import android.os.Looper
import io.github.kuddev.pebrel.terminal.*
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.*
import java.io.InputStream
import java.io.OutputStream
import java.io.PipedInputStream
import java.io.PipedOutputStream
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], application = Application::class, shadows = [TerminalScrollTest.Engine::class],
    instrumentedPackages = ["io.github.kuddev.pebrel.terminal"])
@ConscryptMode(ConscryptMode.Mode.OFF)
class TerminalScrollTest {
    @Implements(value = GhosttyCore::class, isInAndroidSdk = false)
    class Engine {
        @Implementation fun __constructor__(columns: Int, rows: Int, scrollbackLines: Int) = Unit
        @Implementation fun resize(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) = Unit
        @Implementation fun colors(value: IntArray) = Unit
        @Implementation fun close() = Unit
        @Implementation fun synchronizedOutput() = false
        @Implementation fun scroll(lines: Int) = Unit
        @Implementation fun applicationScrollMode(): Int { modeReads.incrementAndGet(); return mode.get() }
        @Implementation fun snapshot() = TerminalFrame(arrayOf(TerminalRow("", intArrayOf())),
            intArrayOf(80, 1, 0, 0, 1, 0, 0, 0, 1, 0, 1))
        @Implementation fun wheel(lines: Int, column: Int, row: Int, applicationOnly: Boolean): ByteArray =
            ByteArray(kotlin.math.abs(lines)) { 65 }
        companion object { val mode = AtomicInteger(2); val modeReads = AtomicInteger() }
    }

    private class Transport : SessionTransport, TerminalHistoryTransport {
        private val input = PipedInputStream()
        private val peer = PipedOutputStream(input)
        val bytes = AtomicInteger()
        val queries = AtomicInteger()
        val observed = AtomicInteger()
        val restored = AtomicInteger()
        val scrolled = AtomicInteger()
        var historyGate: CountDownLatch? = null
        @Volatile var historyFails = false
        override fun open(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) = Unit
        override fun input(): InputStream = input
        override fun output(): OutputStream = object : OutputStream() {
            override fun write(value: Int) { bytes.incrementAndGet() }
            override fun write(value: ByteArray, offset: Int, length: Int) { bytes.addAndGet(length) }
        }
        override fun resize(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) = Unit
        override fun awaitExit(): Int = 0
        override fun close() { peer.close(); input.close() }
        override fun scrollHistory(lines: Int): Boolean {
            queries.incrementAndGet()
            historyGate?.let { check(it.await(3, TimeUnit.SECONDS)) }
            scrolled.addAndGet(lines)
            if (historyFails) throw java.io.IOException("fixture remote history query failed")
            return true
        }
        override fun applicationScrolled() { observed.incrementAndGet() }
        override fun returnToLiveInput() { restored.incrementAndGet() }
    }

    private fun await(condition: () -> Boolean) {
        repeat(500) {
            shadowOf(Looper.getMainLooper()).idle()
            if (condition()) return
            Thread.sleep(5)
        }
        fail("terminal operation did not finish")
    }

    @Test fun mouseReportingUsesTheExistingPtyEvenWhenHistoryQueriesFail() {
        Engine.mode.set(2)
        val transport = Transport().apply { historyFails = true }
        val rejected = AtomicInteger()
        val terminal = TerminalSession(transport, object : TerminalCallbacks() {
            override fun onInputRejected(session: TerminalSession) { rejected.incrementAndGet() }
        })
        try {
            terminal.setVisible(true)
            terminal.start()
            await { terminal.isReady && terminal.frame != null }
            repeat(20) { batch ->
                repeat(8) { terminal.wheel(-3, 2, 4) }
                await { transport.bytes.get() == (batch + 1) * 24 }
            }
            assertTrue(transport.observed.get() in 1..160)
            assertEquals(0, transport.queries.get())
            assertEquals(0, rejected.get())
            assertTrue(terminal.sendText("x"))
            await { transport.bytes.get() == 481 }
            assertEquals(1, transport.restored.get())
        } finally { terminal.finishIfRunning() }
    }

    @Test fun missingMouseReportingKeepsTypedHistoryInsteadOfTypingArrowKeys() {
        Engine.mode.set(1)
        val transport = Transport()
        val terminal = TerminalSession(transport, TerminalCallbacks())
        try {
            terminal.setVisible(true)
            terminal.start()
            await { terminal.isReady && terminal.frame != null }
            terminal.wheel(-3, 2, 4)
            await { transport.queries.get() == 1 }
            assertEquals(0, transport.bytes.get())
            assertEquals(0, transport.observed.get())
        } finally { terminal.finishIfRunning() }
    }

    @Test fun queuedWheelAfterAlternateScreenExitDoesNotQueryOrTypeIntoTheShell() {
        Engine.mode.set(0)
        Engine.modeReads.set(0)
        val transport = Transport()
        val terminal = TerminalSession(transport, TerminalCallbacks())
        try {
            terminal.setVisible(true)
            terminal.start()
            // Snapshot deliberately still says alternate screen, as during a pending repaint.
            await { terminal.isReady && terminal.frame != null }
            terminal.wheel(-3, 2, 4)
            await { Engine.modeReads.get() > 0 }
            assertTrue(terminal.sendText("x"))
            await { transport.bytes.get() == 1 }
            assertEquals(0, transport.queries.get())
            assertEquals(0, transport.observed.get())
        } finally { terminal.finishIfRunning() }
    }

    @Test fun slowFallbackCoalescesGesturesWithoutRejectingInputOrLosingDistance() {
        Engine.mode.set(1)
        val gate = CountDownLatch(1)
        val transport = Transport().apply { historyGate = gate }
        val rejected = AtomicInteger()
        val terminal = TerminalSession(transport, object : TerminalCallbacks() {
            override fun onInputRejected(session: TerminalSession) { rejected.incrementAndGet() }
        })
        try {
            terminal.setVisible(true)
            terminal.start()
            await { terminal.isReady && terminal.frame != null }
            terminal.wheel(-1, 2, 4)
            await { transport.queries.get() == 1 }
            repeat(500) { terminal.wheel(-1, 2, 4) }
            gate.countDown()
            await { transport.scrolled.get() == -501 }
            assertEquals(3, transport.queries.get())
            assertEquals(0, rejected.get())
            assertTrue(terminal.sendText("x"))
            await { transport.bytes.get() == 1 }
        } finally { gate.countDown(); terminal.finishIfRunning() }
    }
}
