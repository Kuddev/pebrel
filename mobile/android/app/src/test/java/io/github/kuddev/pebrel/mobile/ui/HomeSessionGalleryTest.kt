package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.unit.dp
import io.github.kuddev.pebrel.mobile.session.LocalSession
import io.github.kuddev.pebrel.terminal.SessionTransport
import io.github.kuddev.pebrel.terminal.TerminalCallbacks
import io.github.kuddev.pebrel.terminal.TerminalFrame
import io.github.kuddev.pebrel.terminal.TerminalRow
import io.github.kuddev.pebrel.terminal.TerminalSession
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], qualifiers = "w390dp-h844dp-mdpi")
class HomeSessionGalleryTest {
    @get:Rule val compose = createComposeRule()

    @Test fun localPreviewKeepsTerminalOutputAndTheWholeCardOpensItsSession() {
        val terminal = galleryTerminalFixture()
        var opened = ""
        try {
            compose.setContent { MaterialTheme { HomeScreen(
                listOf(LocalSession("local", "Build preview", "Local", terminal, "ready")),
                emptyList(), emptyList(), emptyList(), { opened = it },
                {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {},
            ) } }
            compose.onNodeWithText("$ cargo test\n24 tests passed\n~/workspace $").assertIsDisplayed()
            compose.onNodeWithText("Build preview").assertHeightIsAtLeast(180.dp).assertWidthIsAtLeast(133.dp)
                .performTouchInput { click(Offset(2f, height / 2f)) }
            compose.runOnIdle { assertEquals("local", opened) }
        } finally { terminal.finishIfRunning() }
    }

    @Test fun horizontalGalleryKeepsSessionsBeyondTheFirstFourReachable() {
        val terminal = galleryTerminalFixture()
        var opened = ""
        try {
            compose.setContent { MaterialTheme { HomeScreen(
                (1..6).map { LocalSession("session-$it", "Session $it", "Local", terminal, "ready") },
                emptyList(), emptyList(), emptyList(), { opened = it },
                {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {},
            ) } }
            compose.onNode(SemanticsMatcher.keyIsDefined(SemanticsProperties.HorizontalScrollAxisRange))
                .performScrollToNode(hasText("Session 6"))
            compose.onNodeWithText("Session 6").assertIsDisplayed().performClick()
            compose.runOnIdle { assertEquals("session-6", opened) }
        } finally { terminal.finishIfRunning() }
    }
}

/** A retained terminal frame, with no PTY, polling or native renderer started by the gallery. */
internal fun galleryTerminalFixture(): TerminalSession {
    val transport = object : SessionTransport {
        override fun open(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) = error("Unexpected open")
        override fun input(): java.io.InputStream = error("Unexpected read")
        override fun output(): java.io.OutputStream = error("Unexpected write")
        override fun resize(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) = error("Unexpected resize")
        override fun awaitExit() = 0
        override fun close() = Unit
    }
    val terminal = TerminalSession(transport, TerminalCallbacks())
    val rows = arrayOf<TerminalRow?>(
        TerminalRow("$ cargo test", intArrayOf()),
        TerminalRow("24 tests passed", intArrayOf()),
        TerminalRow("~/workspace $", intArrayOf()),
    )
    val frame = TerminalFrame(rows, intArrayOf(80, 3, 13, 2, 1, 0, 0xffffff, 0))
    TerminalSession::class.java.getDeclaredField("frame").apply { isAccessible = true }.set(terminal, frame)
    return terminal
}
