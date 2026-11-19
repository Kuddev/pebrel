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
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], application = Application::class, shadows = [TerminalViewportTest.Engine::class],
    instrumentedPackages = ["io.github.kuddev.pebrel.terminal"])
@ConscryptMode(ConscryptMode.Mode.OFF)
class TerminalViewportTest {
    @Implements(value = GhosttyCore::class, isInAndroidSdk = false)
    class Engine {
        @Implementation fun __constructor__(columns: Int, rows: Int, scrollbackLines: Int) = Unit
        @Implementation fun resize(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) { height = rows }
        @Implementation fun close() = Unit
        @Implementation fun synchronizedOutput() = false
        @Implementation fun snapshot() = TerminalFrame(Array(height) { TerminalRow("live", intArrayOf()) },
            intArrayOf(80, height, 0, height - 1, 1, 0, 0, 0, height, 0, 1))
        companion object { @Volatile var height = 24 }
    }

    private class Transport : SessionTransport {
        private val closed = CountDownLatch(1)
        val sizes = CopyOnWriteArrayList<Int>()
        @Volatile var initialRows = 0
        @Volatile var gridBeforePty = true
        override fun open(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) { initialRows = rows }
        override fun resize(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) {
            gridBeforePty = gridBeforePty && Engine.height == rows
            sizes.add(rows)
        }
        override fun input() = object : InputStream() {
            override fun read(): Int { closed.await(5, TimeUnit.SECONDS); return -1 }
        }
        override fun output() = OutputStream.nullOutputStream()
        override fun awaitExit() = 0
        override fun close() { closed.countDown() }
    }

    private fun await(condition: () -> Boolean) {
        repeat(500) {
            shadowOf(Looper.getMainLooper()).idle()
            if (condition()) return
            Thread.sleep(5)
        }
        fail("viewport update did not finish")
    }

    @Test fun keyboardAnimationCommitsOnlyTheFinalSizeInGridBeforePtyOrder() {
        val transport = Transport()
        val terminal = TerminalSession(transport, TerminalCallbacks())
        try {
            terminal.updateSize(80, 12, 8, 16)
            terminal.setVisible(true)
            terminal.start()
            await { terminal.isReady && terminal.frame != null }
            Thread.sleep(200)
            transport.sizes.clear()
            // The IME closing animation changes height on separate UI frames.
            for (rows in 13..30) {
                terminal.updateSize(80, rows, 8, 16)
                Thread.sleep(16)
            }
            await { transport.sizes.lastOrNull() == 30 }
            assertEquals(listOf(30), transport.sizes.toList())
            assertTrue(transport.gridBeforePty)
            await { terminal.frame?.rows?.size == 30 }
        } finally { terminal.finishIfRunning() }
    }

    @Test fun firstMeasurementIsUsedEvenWhenItArrivesAfterStart() {
        val transport = Transport()
        val terminal = TerminalSession(transport, TerminalCallbacks())
        try {
            terminal.start()
            terminal.updateSize(80, 35, 8, 16)
            await { terminal.isReady }
            assertEquals(35, transport.initialRows)
            assertEquals(35, Engine.height)
            assertTrue(transport.sizes.isEmpty())
        } finally { terminal.finishIfRunning() }
    }

    @Test fun returningToTheSameTabSizeDoesNotRequestAnotherRemoteRedraw() {
        val transport = Transport()
        val terminal = TerminalSession(transport, TerminalCallbacks())
        try {
            terminal.updateSize(80, 30, 8, 16)
            terminal.setVisible(true)
            terminal.start()
            await { terminal.isReady && terminal.frame != null }
            Thread.sleep(200)
            assertTrue("opening with the measured size must not resize it again", transport.sizes.isEmpty())
            terminal.setVisible(false)
            for (rows in listOf(29, 27, 30)) {
                terminal.updateSize(80, rows, 8, 16)
                Thread.sleep(16)
            }
            terminal.setVisible(true)
            Thread.sleep(250)
            shadowOf(Looper.getMainLooper()).idle()
            assertTrue(transport.sizes.isEmpty())
            assertEquals(30, terminal.frame!!.rows.size)
        } finally { terminal.finishIfRunning() }
    }

    @Test fun closingWithAnUnsettledSizeNeverResizesTheClosedTransport() {
        val transport = Transport()
        val terminal = TerminalSession(transport, TerminalCallbacks())
        try {
            terminal.start()
            await { terminal.isReady }
            Thread.sleep(200)
            transport.sizes.clear()
            terminal.updateSize(80, 40, 8, 16)
            terminal.finishIfRunning()
            Thread.sleep(250)
            assertTrue(transport.sizes.isEmpty())
        } finally { terminal.finishIfRunning() }
    }
}
