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
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.atomic.AtomicInteger

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], application = Application::class, shadows = [TerminalFramePublicationTest.Engine::class],
    instrumentedPackages = ["io.github.kuddev.pebrel.terminal"])
@ConscryptMode(ConscryptMode.Mode.OFF)
class TerminalFramePublicationTest {
    // Exercise the real session reader and publisher with a controllable parser boundary.
    // The native engine's actual DEC mode parsing is covered by its integration test.
    @Implements(value = GhosttyCore::class, isInAndroidSdk = false)
    class Engine {
        @Implementation fun __constructor__(columns: Int, rows: Int, scrollbackLines: Int) {
            text = "latest"; synchronized = false; feeds.set(0); timeouts.set(0)
        }
        @Implementation fun resize(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) = Unit
        @Implementation fun close() = Unit
        @Implementation fun takeTitle(): String? = null
        @Implementation fun feed(bytes: ByteArray, count: Int): ByteArray {
            when (val update = bytes.copyOf(count).toString(Charsets.UTF_8)) {
                "BEGIN" -> synchronized = true
                "END" -> synchronized = false
                else -> text = update
            }
            feeds.incrementAndGet()
            return byteArrayOf()
        }
        @Implementation fun synchronizedOutput(): Boolean = synchronized
        @Implementation fun endSynchronizedOutput() { synchronized = false; timeouts.incrementAndGet() }
        @Implementation fun snapshot() = TerminalFrame(arrayOf(TerminalRow(text, intArrayOf())),
            intArrayOf(80, 1, 0, 0, 1, 0, 0, 0, 1, 0, 1))
        companion object {
            @Volatile var text = "latest"
            @Volatile var synchronized = false
            val feeds = AtomicInteger()
            val timeouts = AtomicInteger()
        }
    }

    private class Transport : SessionTransport {
        private val chunks = LinkedBlockingQueue<ByteArray>()
        fun send(text: String) { chunks.put(text.toByteArray()) }
        fun eof() { chunks.put(byteArrayOf()) }
        override fun open(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) = Unit
        override fun resize(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) = Unit
        override fun input() = object : InputStream() {
            override fun read(): Int = error("fixture expects a bulk read")
            override fun read(bytes: ByteArray, offset: Int, length: Int): Int {
                val chunk = chunks.take()
                if (chunk.isEmpty()) return -1
                require(chunk.size <= length)
                chunk.copyInto(bytes, offset)
                return chunk.size
            }
        }
        override fun output() = OutputStream.nullOutputStream()
        override fun awaitExit() = 0
        override fun close() = eof()
    }

    private fun await(condition: () -> Boolean) {
        repeat(500) {
            shadowOf(Looper.getMainLooper()).idle()
            if (condition()) return
            Thread.sleep(5)
        }
        fail("frame publication did not finish")
    }

    private fun drainFrames() {
        Thread.sleep(80)
        shadowOf(Looper.getMainLooper()).idle()
    }

    @Test fun partialHistoryReplayKeepsTheLastCompleteFrameUntilTheEndMarker() {
        val transport = Transport()
        val terminal = TerminalSession(transport, TerminalCallbacks())
        try {
            terminal.setVisible(true)
            terminal.start()
            await { terminal.frame != null }
            transport.send("BEGIN")
            transport.send("very early history")
            await { Engine.feeds.get() == 2 }
            drainFrames()
            assertEquals("latest", terminal.frame!!.text())
            transport.send("middle of history")
            await { Engine.feeds.get() == 3 }
            drainFrames()
            assertEquals("latest", terminal.frame!!.text())
            transport.send("new latest")
            transport.send("END")
            await { terminal.frame?.text() == "new latest" }
            assertEquals(0, Engine.timeouts.get())
        } finally { terminal.finishIfRunning() }
    }

    @Test fun reattachingDuringReplayDoesNotExposeTheIntermediateHistory() {
        val transport = Transport()
        val terminal = TerminalSession(transport, TerminalCallbacks())
        try {
            terminal.setVisible(true)
            terminal.start()
            await { terminal.frame != null }
            terminal.setVisible(false)
            transport.send("BEGIN")
            transport.send("early history")
            await { Engine.feeds.get() == 2 }
            terminal.setVisible(true)
            drainFrames()
            assertEquals("latest", terminal.frame!!.text())
            transport.send("current bottom")
            transport.send("END")
            await { terminal.frame?.text() == "current bottom" }
        } finally { terminal.finishIfRunning() }
    }

    @Test fun missingEndMarkerPublishesAfterTheDeadlineWithoutAnotherByte() {
        val transport = Transport()
        val terminal = TerminalSession(transport, TerminalCallbacks())
        try {
            terminal.setVisible(true)
            terminal.start()
            await { terminal.frame != null }
            transport.send("BEGIN")
            transport.send("incomplete but no longer arriving")
            await { Engine.feeds.get() == 2 }
            drainFrames()
            assertEquals("latest", terminal.frame!!.text())
            await { terminal.frame?.text() == "incomplete but no longer arriving" }
            assertEquals(1, Engine.timeouts.get())
            transport.send("ordinary streaming output")
            await { terminal.frame?.text() == "ordinary streaming output" }
        } finally { terminal.finishIfRunning() }
    }

    @Test fun eofReleasesTheFinalFrameAndCancelsTheDeadline() {
        val transport = Transport()
        val terminal = TerminalSession(transport, TerminalCallbacks())
        try {
            terminal.setVisible(true)
            terminal.start()
            await { terminal.frame != null }
            transport.send("BEGIN")
            transport.send("final output")
            await { Engine.feeds.get() == 2 }
            drainFrames()
            assertEquals("latest", terminal.frame!!.text())
            transport.eof()
            await { !terminal.isReady && terminal.frame?.text() == "final output" }
            terminal.finishIfRunning()
            val releases = Engine.timeouts.get()
            Thread.sleep(1100)
            assertEquals(releases, Engine.timeouts.get())
        } finally { terminal.finishIfRunning() }
    }

    @Test fun closingDuringReplayDoesNotPublishALateTimeoutFrame() {
        val transport = Transport()
        val published = AtomicInteger()
        val terminal = TerminalSession(transport, object : TerminalCallbacks() {
            override fun onTextChanged(session: TerminalSession) { published.incrementAndGet() }
        })
        try {
            terminal.setVisible(true)
            terminal.start()
            await { terminal.frame != null }
            transport.send("BEGIN")
            transport.send("partial history")
            await { Engine.feeds.get() == 2 }
            drainFrames()
            terminal.finishIfRunning()
            val before = published.get()
            Thread.sleep(1100)
            shadowOf(Looper.getMainLooper()).idle()
            assertEquals(before, published.get())
        } finally { terminal.finishIfRunning() }
    }
}
