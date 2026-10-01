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
    @Test fun scrollbackLimitAndAbsolutePositionUseNativeHistory() = GhosttyCore(40, 6, 1000).use { core ->
        repeat(1200) { core.feed("HISTORY-${it.toString().padStart(4, '0')}\r\n".toByteArray()) }
        val bottom = core.snapshot()
        assertTrue(bottom.text().contains("HISTORY-1199"))
        assertTrue("native history metadata: ${bottom.meta.contentToString()}", bottom.scrollTotal in 1000..1006)
        assertEquals(bottom.scrollTotal - bottom.rows.size, bottom.scrollOffset)
        core.scrollTo(0)
        val top = core.snapshot()
        assertEquals(0, top.scrollOffset)
        assertFalse(top.text().contains("HISTORY-0000"))
        assertFalse(top.text().contains("HISTORY-1199"))
        core.scrollTo(400)
        assertEquals(400, core.snapshot().scrollOffset)
        core.scroll(Int.MAX_VALUE)
        assertTrue(core.snapshot().text().contains("HISTORY-1199"))
    }

    @Test fun pauseEmojiKeepsItsColorAndTextPresentationRemainsMonochrome() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val activity = instrumentation.startActivitySync(checkNotNull(instrumentation.targetContext.packageManager
            .getLaunchIntentForPackage(instrumentation.targetContext.packageName)))
        val context = instrumentation.targetContext
        val paint = android.graphics.Paint().apply {
            typeface = (context.applicationContext as PebrelApplication).terminalTypeface("maple")
            textSize = 16 * context.resources.displayMetrics.scaledDensity
        }
        val width = paint.measureText("M")
        val height = kotlin.math.ceil(paint.fontMetrics.descent - paint.fontMetrics.ascent + paint.fontMetrics.leading)
        val viewRef = java.util.concurrent.atomic.AtomicReference<GhosttyView>()
        val terminal = TerminalSession(LocalPtyTransport(context.filesDir.absolutePath, context.filesDir.resolve("terminal").absolutePath),
            object : TerminalCallbacks() { override fun onTextChanged(session: TerminalSession) { viewRef.get()?.onScreenUpdated() } })
        try {
            instrumentation.runOnMainSync {
                val view = GhosttyView(activity).apply { setFont(paint.typeface, paint.textSize); session = terminal }
                viewRef.set(view); activity.setContentView(view)
            }
            terminal.start(); terminal.setVisible(true)
            await { terminal.isReady }
            terminal.sendText("printf '\\033[2J\\033[H⏸ manual\\r\\n⏸︎ text\\r\\n⏸X\\r\\n'\r")
            await { terminal.frame?.rows?.getOrNull(0)?.text?.trimEnd() == "⏸ manual" &&
                terminal.frame?.rows?.getOrNull(2)?.text?.trimEnd() == "⏸X" }
            instrumentation.runOnMainSync {
                val view = viewRef.get()
                val bitmap = android.graphics.Bitmap.createBitmap(view.width, view.height, android.graphics.Bitmap.Config.ARGB_8888)
                view.draw(android.graphics.Canvas(bitmap))
                fun colored(row: Int, fromColumn: Int = 0, toColumn: Int = 1): Int {
                    var count = 0
                    for (y in (row * height).toInt() until ((row + 1) * height).toInt())
                        for (x in (fromColumn * width).toInt() until (toColumn * width).toInt()) {
                        val color = bitmap.getPixel(x, y)
                        val channels = listOf(android.graphics.Color.red(color), android.graphics.Color.green(color), android.graphics.Color.blue(color))
                        if (channels.max() - channels.min() > 40) count++
                    }
                    return count
                }
                assertTrue("pause button must keep a colored emoji background", colored(0) > 20)
                assertEquals("FE0E explicitly requests text, not a colored button", 0, colored(1))
                assertTrue("use the following blank to keep the color button legible", colored(0, 1, 2) > 20)
                assertEquals("without a blank the adjacent letter must not be covered", 0, colored(2, 1, 2))
                var first = height.toInt()
                var last = 0
                for (y in 0 until height.toInt()) for (x in 0 until (2 * width).toInt()) {
                    val color = bitmap.getPixel(x, y)
                    val channels = listOf(android.graphics.Color.red(color), android.graphics.Color.green(color), android.graphics.Color.blue(color))
                    if (channels.max() - channels.min() > 40) { first = minOf(first, y); last = maxOf(last, y) }
                }
                assertTrue("colored pause must have readable text-height, not a tiny dot: ${last - first + 1} / $height",
                    last - first + 1 >= height * .55f)
                assertTrue("drawing must preserve raw text", terminal.frame!!.rows[0]!!.text.startsWith("⏸ manual"))
                bitmap.recycle()
            }
        } finally { terminal.finishIfRunning(); instrumentation.runOnMainSync { activity.finish() } }
    }

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
        assertEquals("\u001b[Z", core.key(KeyEvent.KEYCODE_TAB, 1, 1).toString(Charsets.UTF_8))
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
        val session = TerminalSession(LocalPtyTransport(target.filesDir.absolutePath, target.filesDir.resolve("terminal").absolutePath), object : TerminalCallbacks() {
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

    @Test fun localListingColorsFileTypesButDoesNotColorRedirectedOutput() {
        val target = InstrumentationRegistry.getInstrumentation().targetContext
        val directory = java.io.File(target.cacheDir, "color-${System.nanoTime()}").apply { mkdirs() }
        java.io.File(directory, "folder").mkdir()
        java.io.File(directory, "note.md").writeText("# Note", Charsets.UTF_8)
        java.io.File(directory, "script.sh").apply { writeText("#!/system/bin/sh\n", Charsets.UTF_8); setExecutable(true) }
        android.system.Os.symlink("note.md", java.io.File(directory, "link").absolutePath)
        val ready = CountDownLatch(1)
        val terminal = TerminalSession(LocalPtyTransport(directory.absolutePath, target.filesDir.resolve("terminal").absolutePath), object : TerminalCallbacks() {
            override fun onTransportReady(session: TerminalSession) { ready.countDown() }
        })
        try {
            terminal.start(); terminal.setVisible(true)
            assertTrue(ready.await(10, TimeUnit.SECONDS))
            // stty 用 TCSAFLUSH 更新终端；同一行先由 shell 解析，后续命令才不会被丢弃。
            assertTrue(terminal.sendText("stty -echo; clear; ls -1\r"))
            try {
                await { terminal.frame?.rows?.any { it?.text?.trim() == "script.sh" } == true }
            } catch (error: AssertionError) {
                throw AssertionError("local listing: ${terminal.frame?.text()} / ${terminal.failure}", error)
            }
            val rows = checkNotNull(terminal.frame).rows.filterNotNull()
            fun color(name: String) = rows.first { it.text.trim() == name }.cells[3]
            val plain = color("note.md")
            assertNotEquals(plain, color("folder"))
            assertNotEquals(plain, color("link"))
            assertNotEquals(plain, color("script.sh"))
            val listing = java.io.File(directory, "listing.txt")
            assertTrue(terminal.sendText("ls -1 > listing.txt\r"))
            await { listing.isFile && listing.length() > 0 }
            assertFalse(listing.readBytes().contains(27.toByte()))
        } finally { terminal.finishIfRunning(); directory.deleteRecursively() }
    }

    @Test fun terminalBackgroundCannotEraseSiblingChrome() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val target = instrumentation.targetContext
        val ready = CountDownLatch(1)
        val terminal = TerminalSession(LocalPtyTransport(target.filesDir.absolutePath, target.filesDir.resolve("terminal").absolutePath), object : TerminalCallbacks() {
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

    @Test fun wordLineCopyAndTapAwayUseRealDeviceTouchEvents() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val target = instrumentation.targetContext
        val device = androidx.test.uiautomator.UiDevice.getInstance(instrumentation)
        val repository = (target.applicationContext as PebrelApplication).sessions
        val existing = repository.sessions.value.map { it.id }.toSet()
        val activity = instrumentation.startActivitySync(checkNotNull(target.packageManager.getLaunchIntentForPackage(target.packageName))
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK))
        instrumentation.waitForIdleSync()
        val labels = listOf(R.string.local_terminal, R.string.home_open_local, R.string.onboarding_phone_terminal)
            .joinToString("|") { java.util.regex.Pattern.quote(target.getString(it)) }
        checkNotNull(device.wait(androidx.test.uiautomator.Until.findObject(
            androidx.test.uiautomator.By.text(java.util.regex.Pattern.compile(labels))), 5000)).click()
        await { repository.sessions.value.any { it.id !in existing && it.source == "Local" && it.status == "ready" } }
        val local = repository.sessions.value.single { it.id !in existing && it.source == "Local" }
        val terminal = local.terminal
        fun findTerminal(view: android.view.View): GhosttyView? {
            if (view is GhosttyView && view.session === terminal) return view
            if (view is android.view.ViewGroup) for (i in 0 until view.childCount) {
                findTerminal(view.getChildAt(i))?.let { return it }
            }
            return null
        }
        val viewRef = java.util.concurrent.atomic.AtomicReference<GhosttyView>()
        await {
            instrumentation.runOnMainSync { viewRef.set(findTerminal(activity.window.decorView)) }
            viewRef.get()?.width?.let { it > 0 } == true
        }
        device.waitForIdle()
        val preferences = repository.display.state.value
        val paint = android.graphics.Paint().apply {
            typeface = (target.applicationContext as PebrelApplication).terminalTypeface(preferences.fontFamily)
            textSize = preferences.fontSize * target.resources.displayMetrics.scaledDensity
        }
        val cellWidth = paint.measureText("M")
        val metrics = paint.fontMetrics
        val cellHeight = kotlin.math.ceil(metrics.descent - metrics.ascent + metrics.leading)
        val location = IntArray(2)
        val copyLabel = target.getString(android.R.string.copy)
        val clipboard = target.getSystemService(android.content.ClipboardManager::class.java)
        val previousClip = clipboard.primaryClip
        fun saveImage(name: String, bitmap: android.graphics.Bitmap) {
            val directory = java.io.File(target.getExternalFilesDir(null), "e2e").apply { mkdirs() }
            java.io.File(directory, "$name.png").outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
        }
        fun capture(name: String) {
            val bitmap = instrumentation.uiAutomation.takeScreenshot()
            saveImage(name, bitmap)
            bitmap.recycle()
        }
        fun tap(x: Float, y: Float, count: Int = 1, hold: Long = 20) {
            repeat(count) {
                val down = android.os.SystemClock.uptimeMillis()
                instrumentation.sendPointerSync(android.view.MotionEvent.obtain(down, down, android.view.MotionEvent.ACTION_DOWN, x, y, 0))
                android.os.SystemClock.sleep(hold)
                instrumentation.sendPointerSync(android.view.MotionEvent.obtain(down, android.os.SystemClock.uptimeMillis(), android.view.MotionEvent.ACTION_UP, x, y, 0))
                android.os.SystemClock.sleep(40)
            }
        }
        fun firstRows(): android.graphics.Bitmap {
            val screen = instrumentation.uiAutomation.takeScreenshot()
            return try { android.graphics.Bitmap.createBitmap(screen, location[0], location[1],
                viewRef.get().width, (cellHeight * 2).toInt()) }
            finally { screen.recycle() }
        }
        fun expectRowsRestored(expected: android.graphics.Bitmap, name: String) {
            // ActionMode 的退场动画晚于剪贴板回调，比较最终像素而非刚发出关闭请求的帧。
            try { await { firstRows().let { pixels ->
                pixels.sameAs(expected).also { pixels.recycle() }
            } } } finally { firstRows().let { pixels -> saveImage(name, pixels); pixels.recycle() } }
        }
        fun awaitDraw() {
            val drawn = CountDownLatch(1)
            instrumentation.runOnMainSync { viewRef.get().postOnAnimation { viewRef.get().postOnAnimation { drawn.countDown() } } }
            assertTrue(drawn.await(3, TimeUnit.SECONDS))
        }
        var baseline: android.graphics.Bitmap? = null
        try {
            // 点空白退出选择也会唤起输入法；先用真实点击打开键盘，避免把窗口缩放当成选区残留。
            instrumentation.runOnMainSync { viewRef.get().getLocationOnScreen(location) }
            tap(location[0] + viewRef.get().width * .8f, location[1] + viewRef.get().height * .6f)
            await {
                var visible = false
                instrumentation.runOnMainSync {
                    visible = androidx.core.view.ViewCompat.getRootWindowInsets(viewRef.get())
                        ?.isVisible(androidx.core.view.WindowInsetsCompat.Type.ime()) == true
                }
                visible
            }
            device.waitForIdle()
            assertTrue(terminal.sendText("printf '\\033[2J\\033[Halpha beta\\r\\nsecond line\\r\\n'\r"))
            // 命令回显也含 alpha beta；必须等到真正执行后的两行，才建立像素基线。
            await { terminal.frame?.rows?.getOrNull(0)?.text?.trimEnd() == "alpha beta" &&
                terminal.frame?.rows?.getOrNull(1)?.text?.trimEnd() == "second line" }
            awaitDraw()
            device.waitForIdle()
            instrumentation.runOnMainSync { viewRef.get().getLocationOnScreen(location) }
            baseline = firstRows()
            saveImage("selection-baseline-rows", baseline)
            val x = location[0] + cellWidth * 2.5f
            val y = location[1] + cellHeight * .4f
            for ((count, expected) in listOf(2 to "alpha", 3 to "alpha beta")) {
                android.os.SystemClock.sleep(400)
                tap(x, y, count)
                val copy = device.wait(androidx.test.uiautomator.Until.findObject(androidx.test.uiautomator.By.text(copyLabel)), 3000)
                assertNotNull("copy toolbar for $count taps", copy)
                capture(if (count == 2) "word-selection" else "line-selection")
                // 三击会移动浮动菜单；等布局稳定后重新定位，并等待系统实际完成剪贴板写入。
                device.waitForIdle()
                checkNotNull(device.findObject(androidx.test.uiautomator.By.text(copyLabel))).click()
                await { clipboard.primaryClip?.getItemAt(0)?.coerceToText(target)?.toString() == expected }
                assertEquals(expected, clipboard.primaryClip?.getItemAt(0)?.coerceToText(target)?.toString())
                expectRowsRestored(baseline, "selection-after-copy-$count-rows")
            }
            android.os.SystemClock.sleep(400)
            tap(x, y, hold = android.view.ViewConfiguration.getLongPressTimeout().toLong() + 150)
            assertTrue(device.wait(androidx.test.uiautomator.Until.hasObject(androidx.test.uiautomator.By.text(copyLabel)), 3000))
            tap(location[0] + viewRef.get().width * .8f, location[1] + viewRef.get().height * .6f)
            assertTrue("tap away must close the copy toolbar", device.wait(androidx.test.uiautomator.Until.gone(androidx.test.uiautomator.By.text(copyLabel)), 3000))
            assertEquals("alpha beta", clipboard.primaryClip?.getItemAt(0)?.coerceToText(target)?.toString())
            expectRowsRestored(baseline, "selection-after-dismiss-rows")
            capture("selection-dismissed")
            // read 会改变规范模式/回车翻译；完整恢复原 termios，不能只恢复 echo 后丢掉下一次 Enter。
            assertTrue(terminal.sendText("tty_state=\$(stty -g); stty -echo -iuclc; printf '\\120ASTE_READY\\n'; IFS= read -r value; stty \"\$tty_state\"; printf '\\120ASTE_E2E:%s\\n' \"\$value\"\r"))
            await { terminal.frame?.text()?.contains("PASTE_READY") == true }
            tap(x, y, hold = android.view.ViewConfiguration.getLongPressTimeout().toLong() + 150)
            val pasteLabel = target.getString(android.R.string.paste)
            assertTrue(device.wait(androidx.test.uiautomator.Until.hasObject(androidx.test.uiautomator.By.text(pasteLabel)), 3000))
            device.waitForIdle()
            checkNotNull(device.findObject(androidx.test.uiautomator.By.text(pasteLabel))).click()
            // UiAutomator 注入点击后即返回；先确认粘贴回调已执行，避免 Enter 抢在粘贴前入队。
            expectRowsRestored(baseline, "selection-after-paste-rows")
            assertTrue(terminal.key(KeyEvent.KEYCODE_ENTER))
            await { terminal.frame?.text()?.contains("PASTE_E2E:alpha beta") == true }
            awaitDraw()
            capture("clipboard-pasted-to-pty")
            // 像实际输入一样，等命令已进入行编辑器再单独按 Enter；不要让终端模式切换吞掉提交键。
            assertTrue(terminal.sendText("printf '\\033[3J\\033[2J\\033[H'; seq -f SCROLL_%04g 0 399"))
            await { terminal.frame?.text()?.contains("399") == true }
            assertTrue(terminal.key(KeyEvent.KEYCODE_ENTER))
            try { await { terminal.frame?.text()?.contains("SCROLL_0399") == true } }
            catch (error: AssertionError) {
                capture("scrollbar-output-failure")
                throw AssertionError("scroll fixture output: ${terminal.frame?.text()?.take(2400)}", error)
            }
            awaitDraw()
            val bottom = checkNotNull(terminal.frame)
            assertTrue(bottom.scrollTotal > bottom.rows.size)
            val density = target.resources.displayMetrics.density
            val track = viewRef.get().height - 12 * density
            val thumb = maxOf(48 * density, track * bottom.rows.size / bottom.scrollTotal)
            val railX = location[0] + viewRef.get().width - 4 * density
            val endY = location[1] + viewRef.get().height - 6 * density - thumb / 2
            val topY = location[1] + 6 * density + thumb / 2
            val dragTime = android.os.SystemClock.uptimeMillis()
            for ((action, y) in listOf(android.view.MotionEvent.ACTION_DOWN to endY,
                android.view.MotionEvent.ACTION_MOVE to topY, android.view.MotionEvent.ACTION_UP to topY)) {
                val event = android.view.MotionEvent.obtain(dragTime, android.os.SystemClock.uptimeMillis(), action, railX, y, 0)
                instrumentation.sendPointerSync(event); event.recycle()
                android.os.SystemClock.sleep(40)
            }
            await { terminal.frame?.scrollOffset == 0 && terminal.frame?.text()?.contains("SCROLL_0000") == true }
            awaitDraw()
            capture("history-scrollbar-drag-top")
        } finally {
            baseline?.recycle()
            instrumentation.runOnMainSync {
                repository.closeTerminal(local.id)
                if (previousClip != null) clipboard.setPrimaryClip(previousClip)
                activity.finish()
            }
        }
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
