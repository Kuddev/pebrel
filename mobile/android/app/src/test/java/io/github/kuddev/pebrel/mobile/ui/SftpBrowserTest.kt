package io.github.kuddev.pebrel.mobile.ui

import androidx.activity.OnBackPressedDispatcher
import androidx.activity.compose.LocalOnBackPressedDispatcherOwner
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.hapticfeedback.HapticFeedback
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.test.core.app.ApplicationProvider
import io.github.kuddev.pebrel.mobile.PebrelApplication
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.SftpClient
import io.github.kuddev.pebrel.mobile.session.LocalSession
import io.github.kuddev.pebrel.mobile.session.SessionRepository
import io.github.kuddev.pebrel.terminal.*
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class SftpBrowserTest {
    @get:Rule val compose = createComposeRule()
    private val context get() = ApplicationProvider.getApplicationContext<PebrelApplication>()
    private var back: OnBackPressedDispatcher? = null
    private var exits = 0
    private var opened = 0
    private val reads = java.util.concurrent.atomic.AtomicInteger()
    private val closes = java.util.concurrent.atomic.AtomicInteger()
    private val listedPaths = java.util.concurrent.CopyOnWriteArrayList<String>()
    private val loadedPaths = java.util.concurrent.CopyOnWriteArrayList<String>()
    private var renderedView: android.view.View? = null
    private val haptics = mutableListOf<HapticFeedbackType>()
    private val feedback = object : HapticFeedback {
        override fun performHapticFeedback(hapticFeedbackType: HapticFeedbackType) { haptics += hapticFeedbackType }
    }

    private fun newTerminal() = TerminalSession(object : SessionTransport {
            override fun open(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) = Unit
            override fun resize(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) = Unit
            override fun input() = ByteArrayInputStream(byteArrayOf())
            override fun output() = ByteArrayOutputStream()
            override fun awaitExit() = 0
            override fun close() = Unit
        }, TerminalCallbacks())

    private fun showFiles(cursor: Long? = null, initialPath: String = "/fixture", directories: Boolean = false): TerminalSession {
        val terminal = newTerminal()
        val client = SftpClient({ request ->
            if (request.getString("op") == "close_list") {
                assertEquals(cursor, request.getLong("cursor"))
                closes.incrementAndGet()
                return@SftpClient JSONObject()
            }
            check(request.getString("op") == "list")
            reads.incrementAndGet()
            val path = request.getString("path")
            listedPaths += path
            val rows = JSONArray()
            if (directories) rows.put(JSONObject().put("path", "$path/nested").put("name", "nested")
                .put("kind", "directory").put("revision", "nested"))
            for (name in listOf("A.txt", "B.txt", "C.txt", "D.txt")) rows.put(JSONObject()
                .put("path", "${path.trimEnd('/')}/$name").put("name", name).put("kind", "file").put("size", 128)
                .put("modified", 1790726400L).put("permissions", 420).put("revision", name))
            JSONObject().put("path", path).put("entries", rows)
                .put("cursor", if (reads.get() == 1) cursor ?: JSONObject.NULL else JSONObject.NULL).put("skipped", 0)
        })
        compose.setContent {
            renderedView = LocalView.current
            back = LocalOnBackPressedDispatcherOwner.current?.onBackPressedDispatcher
            CompositionLocalProvider(LocalHapticFeedback provides feedback) {
                MaterialTheme { SftpBrowserScreen(LocalSession("sftp-ui", "Fixture", "SSH", terminal,
                    status = "ready", files = client), initialPath, { loadedPaths += it }, { exits++ }, { opened++ }) }
            }
        }
        compose.waitUntil(5000) { compose.onAllNodesWithText("A.txt").fetchSemanticsNodes().isNotEmpty() }
        return terminal
    }

    private fun swipe(name: String, right: Boolean = false) {
        compose.onNode(hasText(name) and hasClickAction()).performTouchInput {
            if (right) swipeRight(durationMillis = 300) else swipeLeft(durationMillis = 300)
        }
    }

    private fun awaitDirectory(path: String) {
        // Compose 空闲不代表 IO 已完成；等列表提交回调，再读取下一层真实布局。
        compose.waitForIdle()
        compose.waitUntil(5000) { loadedPaths.lastOrNull() == path }
        compose.waitForIdle()
    }

    @Test fun departingTerminalReleasesInputBeforeTheExitAnimationDisposesItsView() {
        val terminal = newTerminal()
        val repository = SessionRepository(context)
        val active = mutableStateOf(true)
        try {
            compose.setContent {
                renderedView = LocalView.current
                MaterialTheme { LocalTerminalScreen(LocalSession("leaving", "Fixture", "SSH", terminal, status = "ready"),
                    repository, {}, {}, {}, {}, {}, active = active.value) }
            }
            fun find(view: android.view.View): GhosttyView? {
                if (view is GhosttyView) return view
                if (view is android.view.ViewGroup) for (i in 0 until view.childCount) find(view.getChildAt(i))?.let { return it }
                return null
            }
            lateinit var view: GhosttyView
            compose.runOnIdle {
                view = checkNotNull(find(checkNotNull(renderedView).rootView))
                assertTrue(view.directInput)
                view.requestFocus()
                active.value = false
            }
            compose.runOnIdle {
                assertTrue("outgoing view is still attached during the transition", view.isAttachedToWindow)
                assertFalse(view.directInput)
                assertFalse(view.hasFocus())
                active.value = true
            }
            compose.runOnIdle {
                assertTrue("returning restores input without changing the saved preference", view.directInput)
                assertTrue(repository.display.state.value.directInput)
            }
        } finally { terminal.finishIfRunning() }
    }

    @Test fun bothSwipeDirectionsSelectTheInclusiveRangeWithOneHapticEachAndBackClears() {
        val terminal = showFiles()
        try {
            swipe("A.txt", right = true); swipe("D.txt")
            for (name in listOf("A.txt", "B.txt", "C.txt", "D.txt")) {
                compose.onNode(hasText(name) and hasClickAction()).assertIsSelected()
            }
            assertEquals(0, opened)
            assertEquals(listOf(HapticFeedbackType.LongPress, HapticFeedbackType.LongPress), haptics)
            compose.runOnIdle { checkNotNull(back).onBackPressed() }
            compose.onNode(hasText("A.txt") and hasClickAction()).assertIsNotSelected()
            assertEquals(0, exits)
            swipe("D.txt"); swipe("B.txt", right = true)
            assertEquals(4, haptics.size)
            compose.onNode(hasText("A.txt") and hasClickAction()).assertIsNotSelected()
            for (name in listOf("B.txt", "C.txt", "D.txt")) compose.onNode(hasText(name) and hasClickAction()).assertIsSelected()
            compose.onNodeWithContentDescription(context.getString(R.string.more_actions)).performClick()
            compose.onNodeWithText(context.getString(R.string.sftp_refresh)).assertIsDisplayed()
            compose.onNode(hasText("B.txt") and hasClickAction()).assertIsNotSelected()
        } finally { terminal.finishIfRunning() }
    }

    @Test fun addButtonClearsSelectionAndStillOpensItsMenu() {
        val terminal = showFiles()
        try {
            swipe("A.txt")
            compose.onNodeWithContentDescription(context.getString(R.string.sftp_add)).performClick()
            compose.onNodeWithText(context.getString(R.string.sftp_upload)).assertIsDisplayed()
            compose.onNode(hasText("A.txt") and hasClickAction()).assertIsNotSelected()
            assertEquals(0, opened)
        } finally { terminal.finishIfRunning() }
    }

    @Test fun rangeDoesNotIncludeHiddenOrMissingPaths() {
        val visible = listOf("A", "B", "D")
        assertEquals(setOf("A", "B", "D"), sftpSelectionRange(visible, "A", "D"))
        assertEquals(setOf("B"), sftpSelectionRange(visible, "gone", "B"))
        assertTrue(sftpSelectionRange(visible, "A", "hidden").isEmpty())
    }

    @Test fun parentIsNavigationOnlyEvenDuringRangeSelection() {
        val terminal = showFiles(initialPath = "/fixture/one/two")
        try {
            val parent = compose.onNode(hasText("..") and hasClickAction())
            for ((right, destination) in listOf(false to "/fixture/one", true to "/fixture")) {
                swipe("D.txt"); swipe("A.txt")
                compose.onAllNodes(isSelected()).assertCountEquals(4)
                parent.assert(SemanticsMatcher.keyNotDefined(SemanticsProperties.Selected))
                swipe("..", right)
                awaitDirectory(destination)
                assertEquals("parent swipe navigates instead of selecting", destination, listedPaths.last())
                compose.onAllNodes(isSelected()).assertCountEquals(0)
            }
            // 父目录是导航，不是批量操作对象；多选时点击它仍换目录并清空选区。
            parent.performClick()
            awaitDirectory("/")
            assertEquals("parent click requests the parent directory", "/", listedPaths.last())
            compose.onAllNodes(isSelected()).assertCountEquals(0)
            compose.onNodeWithText("..").assertDoesNotExist()
            assertEquals(0, opened)
            assertEquals(0, exits)
        } finally { terminal.finishIfRunning() }
    }

    @Test fun pageBackExitsOnceRegardlessOfDirectoryDepthAndSelectionStillCancelsFirst() {
        val terminal = showFiles(directories = true)
        try {
            repeat(3) { depth ->
                compose.onNode(hasText("nested") and hasClickAction() and isNotSelected()).performClick()
                awaitDirectory("/fixture" + "/nested".repeat(depth + 1))
            }
            assertEquals("/fixture/nested/nested/nested", listedPaths.last())
            val readsBeforeBack = reads.get()
            swipe("A.txt")
            compose.runOnIdle { checkNotNull(back).onBackPressed() }
            compose.onAllNodes(isSelected()).assertCountEquals(0)
            assertEquals(0, exits)
            compose.runOnIdle { checkNotNull(back).onBackPressed() }
            assertEquals(1, exits)
            assertEquals("page back must not relist a previously visited directory", readsBeforeBack, reads.get())
            compose.onNodeWithContentDescription(context.getString(R.string.back)).performClick()
            assertEquals(2, exits)
        } finally { terminal.finishIfRunning() }
    }

    @Test fun fileContentFollowsTheFingerBeforeReleaseAndCancelledOrShortDragsRebound() {
        val terminal = showFiles()
        try {
            val row = compose.onNode(hasText("A.txt") and hasClickAction())
            val label = compose.onNodeWithText("A.txt", useUnmergedTree = true)
            val density = context.resources.displayMetrics.density
            for (direction in listOf(-1f, 1f)) {
                // 取拖动方向的后沿，避免把容器裁剪误判成停止跟手。
                fun edge(): Float = label.fetchSemanticsNode().boundsInRoot.let { if (direction < 0) it.right else it.left }
                val original = edge()
                row.performTouchInput {
                    down(Offset(width * (if (direction < 0) .75f else .25f), centerY))
                    moveBy(Offset(direction * 40f * density, 0f))
                }
                val first = edge()
                assertTrue("content must follow the finger: $original -> $first", (first - original) * direction > 12f * density)
                row.assertIsNotSelected()
                row.performTouchInput { moveBy(Offset(direction * 24f * density, 0f)) }
                val second = edge()
                assertEquals("content follows subsequent finger movement", 24f * density, (second - first) * direction, density)
                row.performTouchInput { cancel() }
                assertEquals(original, edge(), 1f)
                row.assertIsNotSelected()
                row.performTouchInput {
                    down(Offset(width * (if (direction < 0) .75f else .25f), centerY))
                    moveBy(Offset(direction * 32f * density, 0f))
                    up()
                }
                row.assertIsNotSelected()
                assertEquals(original, edge(), 1f)
            }
            assertTrue("short or cancelled drags must not vibrate", haptics.isEmpty())
            assertEquals(0, opened)
        } finally { terminal.finishIfRunning() }
    }

    @Test fun adjacentSelectedRowsPaintOneFullWidthRectangularStrip() {
        val terminal = showFiles()
        try {
            swipe("A.txt"); swipe("D.txt")
            val list = compose.onNodeWithTag("sftp-file-list").fetchSemanticsNode().boundsInRoot
            val bounds = listOf("A.txt", "B.txt", "C.txt", "D.txt").map {
                compose.onNode(hasText(it) and hasClickAction()).fetchSemanticsNode().boundsInRoot
            }
            bounds.forEach {
                assertEquals("selection reaches the left list edge", list.left, it.left, .5f)
                assertEquals("selection reaches the right list edge", list.right, it.right, .5f)
            }
            bounds.zipWithNext().forEach { (a, b) -> assertEquals(a.bottom, b.top, .5f) }
            compose.runOnIdle {
                val view = checkNotNull(renderedView)
                val bitmap = android.graphics.Bitmap.createBitmap(view.width, view.height, android.graphics.Bitmap.Config.ARGB_8888)
                try {
                    view.draw(android.graphics.Canvas(bitmap))
                    val left = list.left.toInt() + 1
                    val right = list.right.toInt() - 2
                    val expected = bitmap.getPixel(left, bounds.first().center.y.toInt())
                    assertTrue("selection must actually be painted", android.graphics.Color.alpha(expected) > 0)
                    for (y in bounds.first().top.toInt() + 1 until bounds.last().bottom.toInt() - 1) {
                        assertEquals("continuous left edge at $y", expected, bitmap.getPixel(left, y))
                        assertEquals("continuous right edge at $y", expected, bitmap.getPixel(right, y))
                    }
                } finally { bitmap.recycle() }
            }
        } finally { terminal.finishIfRunning() }
    }

    @Test fun pullingAtTheTopRefreshesTheCurrentDirectoryAndClearsSelection() {
        val terminal = showFiles(cursor = 42)
        try {
            swipe("A.txt")
            compose.onNodeWithTag("sftp-file-list").performTouchInput { swipeDown(durationMillis = 500) }
            compose.waitUntil(5000) { reads.get() == 2 }
            assertEquals(1, closes.get())
            compose.onNode(hasText("A.txt") and hasClickAction()).assertIsNotSelected()
            assertEquals(0, exits)
        } finally { terminal.finishIfRunning() }
    }

    @Test fun fileSizeSitsNextToItsUpdatedDate() {
        val terminal = showFiles()
        try {
            val date = java.text.SimpleDateFormat("yyyy-MM-dd HH:mm", java.util.Locale.getDefault())
                .format(java.util.Date(1790726400L * 1000))
            val size = android.text.format.Formatter.formatShortFileSize(context, 128)
            val dateBounds = compose.onAllNodesWithText(date, useUnmergedTree = true).onFirst().fetchSemanticsNode().boundsInRoot
            val sizeBounds = compose.onAllNodesWithText(size, useUnmergedTree = true).onFirst().fetchSemanticsNode().boundsInRoot
            val gap = (sizeBounds.left - dateBounds.right) / context.resources.displayMetrics.density
            assertTrue("size must follow date with a small gap: $gap dp", gap in 4f..12f)
        } finally { terminal.finishIfRunning() }
    }
}
