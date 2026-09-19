package io.github.kuddev.pebrel.mobile

import android.view.KeyEvent
import android.content.Intent
import android.view.inputmethod.EditorInfo
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.kuddev.pebrel.terminal.*
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.IOException
import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.io.OutputStream
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

/** Exercises the actual JNI library in the optimized APK, not a mocked parser. */
@RunWith(AndroidJUnit4::class)
class GhosttyEngineTest {
    @Test fun utf8ChunksWideCellsAndCombiningCharacters() = GhosttyCore(24, 6).use { core ->
        val bytes = "A中文 e\u0301 😀".toByteArray()
        bytes.forEach { core.feed(byteArrayOf(it)) }
        val frame = core.snapshot()
        assertTrue(frame.text().startsWith("A中文 e\u0301 😀"))
        val cells = requireNotNull(frame.rows[0]).cells
        assertEquals(2, cells[1 * 6 + 2])
        assertEquals(0, cells[2 * 6 + 2])
    }

    @Test fun trueColorInverseAndDirtyRows() = GhosttyCore(20, 6).use { core ->
        core.feed("\u001b[38;2;10;20;30mX\u001b[0m\r\nsecond".toByteArray())
        val first = core.snapshot()
        assertEquals(0xff0a141e.toInt(), requireNotNull(first.rows[0]).cells[3])
        val stable = core.snapshot()
        assertSame(first.rows[0], stable.rows[0])
        core.feed("!".toByteArray())
        val changed = core.snapshot()
        assertTrue(changed.text().contains("second!"))
        assertEquals(first.rows[0]?.text, changed.rows[0]?.text)
    }

    @Test fun alternateScreenResizeAndRestore() = GhosttyCore(20, 6).use { core ->
        core.feed("primary\u001b[?1049h\u001b[Hfull-screen".toByteArray())
        assertTrue(core.snapshot().text().contains("full-screen"))
        core.resize(30, 8, 10, 20)
        core.feed("\u001b[?1049l".toByteArray())
        val frame = core.snapshot()
        assertEquals(30, frame.columns)
        assertEquals(8, frame.rows.size)
        assertTrue(frame.text().contains("primary"))
        assertFalse(frame.text().contains("full-screen"))
    }

    @Test fun terminalQueriesAndTitleReachHost() = GhosttyCore(20, 6).use { core ->
        val reply = core.feed("abc\u001b[6n\u001b]2;测试标题\u0007".toByteArray()).toString(Charsets.UTF_8)
        assertEquals("\u001b[1;4R", reply)
        assertEquals("测试标题", core.takeTitle())
        assertNull(core.takeTitle())
    }

    @Test fun keyEncodingTracksApplicationModeAndPasteCannotEscape() = GhosttyCore().use { core ->
        assertEquals("\u001b[A", core.key(KeyEvent.KEYCODE_DPAD_UP, 0, 1).toString(Charsets.UTF_8))
        core.feed("\u001b[?1h\u001b[?2004h".toByteArray())
        assertEquals("\u001bOA", core.key(KeyEvent.KEYCODE_DPAD_UP, 0, 1).toString(Charsets.UTF_8))
        assertEquals("\u0003", core.key(KeyEvent.KEYCODE_C, 2, 1, "c", 99).toString(Charsets.UTF_8))
        assertEquals("\u001b[200~中[201~文\u001b[201~", core.paste("中\u001b[201~文").toString(Charsets.UTF_8))
    }

    @Test fun scrollbackAndNativeLifetime() {
        repeat(20) {
            val core = GhosttyCore(20, 4)
            core.feed((0..30).joinToString("\r\n") { "line-$it" }.toByteArray())
            assertTrue(core.snapshot().text().contains("line-30"))
            core.scroll(-20)
            assertFalse(core.snapshot().text().contains("line-30"))
            core.scroll(Int.MAX_VALUE)
            assertTrue(core.snapshot().text().contains("line-30"))
            core.close()
            core.close()
            assertTrue(runCatching { core.snapshot() }.isFailure)
        }
    }

    @Test fun localPtyRunsCommandThroughGhostty() {
        val target = InstrumentationRegistry.getInstrumentation().targetContext
        val ready = CountDownLatch(1)
        val session = TerminalSession(LocalPtyTransport(target.filesDir.absolutePath), object : TerminalCallbacks() {
            override fun onTransportReady(session: TerminalSession) { ready.countDown() }
        })
        try {
            session.start()
            session.setVisible(true)
            assertTrue(ready.await(10, TimeUnit.SECONDS))
            // octal prevents the marker appearing solely because shell input was echoed.
            assertTrue(session.sendText("printf '\\107\\110\\117\\123\\124\\124\\131_OK\\n'\r"))
            await { session.frame?.text()?.contains("GHOSTTY_OK") == true }
            assertNull(session.failure)
        } finally { session.finishIfRunning() }
    }

    @Test fun terminalBackgroundCannotEraseSiblingChrome() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val target = instrumentation.targetContext
        val ready = CountDownLatch(1)
        val terminal = TerminalSession(LocalPtyTransport(target.filesDir.absolutePath), object : TerminalCallbacks() {
            override fun onTransportReady(session: TerminalSession) { ready.countDown() }
        })
        try {
            terminal.start()
            terminal.setVisible(true)
            assertTrue(ready.await(8, TimeUnit.SECONDS))
            await { terminal.frame != null }
            instrumentation.runOnMainSync {
                val bitmap = android.graphics.Bitmap.createBitmap(240, 240, android.graphics.Bitmap.Config.ARGB_8888)
                bitmap.eraseColor(android.graphics.Color.MAGENTA)
                val canvas = android.graphics.Canvas(bitmap)
                val view = GhosttyView(target)
                view.layout(0, 0, 180, 100)
                view.session = terminal
                canvas.translate(20f, 80f)
                view.draw(canvas)
                assertEquals(android.graphics.Color.MAGENTA, bitmap.getPixel(100, 20))
                assertEquals(android.graphics.Color.MAGENTA, bitmap.getPixel(100, 210))
                assertNotEquals(android.graphics.Color.MAGENTA, bitmap.getPixel(100, 130))
                view.session = null
                bitmap.recycle()
            }
        } finally { terminal.finishIfRunning() }
    }

    @Test fun rejectedOutputEndsSessionAndRejectsFutureInput() {
        val ready = CountDownLatch(1)
        val finished = CountDownLatch(1)
        val exits = AtomicInteger()
        val transport = BlockingTransport(rejectOutput = true)
        val session = TerminalSession(transport, object : TerminalCallbacks() {
            override fun onTransportReady(session: TerminalSession) { ready.countDown() }
            override fun onSessionFinished(session: TerminalSession) { exits.incrementAndGet(); finished.countDown() }
        })
        try {
            session.start()
            assertTrue(ready.await(8, TimeUnit.SECONDS))
            assertTrue(session.sendText("not-retried"))
            assertTrue(finished.await(8, TimeUnit.SECONDS))
            assertFalse(session.sendText("later"))
            assertEquals(1, exits.get())
            assertNotNull(session.failure)
        } finally { session.finishIfRunning() }
    }

    @Test fun boundedInputAndCloseRejectFurtherWrites() {
        val ready = CountDownLatch(1)
        val transport = BlockingTransport(rejectOutput = false)
        val session = TerminalSession(transport, object : TerminalCallbacks() {
            override fun onTransportReady(session: TerminalSession) { ready.countDown() }
        })
        try {
            assertFalse(session.sendText("before-connect"))
            session.start()
            assertTrue(ready.await(8, TimeUnit.SECONDS))
            val block = ByteArray(32768)
            assertTrue(session.tryWrite(block, 0, block.size))
            assertTrue(transport.writing.await(8, TimeUnit.SECONDS))
            var rejected = false
            repeat(8) { if (!session.tryWrite(block, 0, block.size)) rejected = true }
            assertTrue(rejected)
            session.finishIfRunning()
            assertFalse(session.sendText("closed"))
            assertTrue(transport.closed.await(8, TimeUnit.SECONDS))
        } finally { session.finishIfRunning() }
    }

    @Test fun imePreeditIsLocalAndDetachedInputCannotReachSession() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val target = instrumentation.targetContext
        val ready = CountDownLatch(1)
        val closed = CountDownLatch(1)
        val output = ByteArrayOutputStream()
        val transport = object : SessionTransport {
            override fun open(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) {}
            override fun resize(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) {}
            override fun input() = object : InputStream() { override fun read(): Int { closed.await(); return -1 } }
            override fun output(): OutputStream = output
            override fun awaitExit() = 0
            override fun close() { closed.countDown() }
        }
        val session = TerminalSession(transport, object : TerminalCallbacks() {
            override fun onTransportReady(session: TerminalSession) { ready.countDown() }
        })
        val activity = instrumentation.startActivitySync(Intent(target, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        try {
            session.start()
            assertTrue(ready.await(8, TimeUnit.SECONDS))
            lateinit var view: GhosttyView
            lateinit var connection: android.view.inputmethod.InputConnection
            instrumentation.runOnMainSync {
                view = GhosttyView(activity).apply { this.session = session; directInput = true }
                activity.setContentView(view)
            }
            instrumentation.waitForIdleSync()
            instrumentation.runOnMainSync {
                connection = requireNotNull(view.onCreateInputConnection(EditorInfo()))
                assertTrue(connection.setComposingText("zhong", 1))
                assertTrue(connection.setComposingText("中文", 1))
                assertEquals(0, output.size())
                assertTrue(connection.commitText("中文", 1))
            }
            await { output.toString("UTF-8") == "中文" }
            instrumentation.runOnMainSync {
                activity.setContentView(android.view.View(activity))
                assertFalse(connection.commitText("must-not-send", 1))
            }
            assertEquals("中文", output.toString("UTF-8"))
        } finally {
            instrumentation.runOnMainSync { activity.finish() }
            session.finishIfRunning()
        }
    }

    @Test fun closingDuringConnectDoesNotPublishLateReady() {
        val opening = CountDownLatch(1)
        val closed = CountDownLatch(1)
        val ready = AtomicInteger()
        val transport = object : SessionTransport {
            override fun open(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) { opening.countDown(); closed.await() }
            override fun resize(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) {}
            override fun input(): InputStream = error("Closed connect must not read")
            override fun output(): OutputStream = error("Closed connect must not write")
            override fun awaitExit() = 0
            override fun close() { closed.countDown() }
        }
        val session = TerminalSession(transport, object : TerminalCallbacks() {
            override fun onTransportReady(session: TerminalSession) { ready.incrementAndGet() }
        })
        session.start()
        assertTrue(opening.await(8, TimeUnit.SECONDS))
        session.finishIfRunning()
        assertTrue(closed.await(8, TimeUnit.SECONDS))
        InstrumentationRegistry.getInstrumentation().runOnMainSync { assertEquals(0, ready.get()) }
        assertFalse(session.sendText("late"))
    }

    private fun await(condition: () -> Boolean) {
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(8)
        while (!condition() && System.nanoTime() < deadline) Thread.sleep(25)
        assertTrue(condition())
    }

    private class BlockingTransport(private val rejectOutput: Boolean) : SessionTransport {
        val closed = CountDownLatch(1)
        val writing = CountDownLatch(1)
        override fun open(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) {}
        override fun resize(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) {}
        override fun input() = object : InputStream() { override fun read(): Int { closed.await(); return -1 } }
        override fun output() = object : OutputStream() {
            override fun write(value: Int) { writing.countDown(); if (rejectOutput) throw IOException("rejected"); closed.await() }
            override fun write(bytes: ByteArray, offset: Int, length: Int) = write(0)
        }
        override fun awaitExit() = 0
        override fun close() { closed.countDown() }
    }
}
