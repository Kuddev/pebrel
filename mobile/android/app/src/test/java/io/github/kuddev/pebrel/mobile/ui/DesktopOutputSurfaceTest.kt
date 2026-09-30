package io.github.kuddev.pebrel.mobile.ui

import android.view.View
import android.view.ViewGroup
import android.view.HapticFeedbackConstants
import android.view.inputmethod.EditorInfo
import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.dp
import io.github.kuddev.pebrel.terminal.TerminalInputTarget
import io.github.kuddev.pebrel.terminal.TerminalSnapshotView
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class DesktopOutputSurfaceTest {
    @get:Rule val compose = createComposeRule()

    @Test fun legacyTextKeepsTheNativeKeyboardTargetAndRevokesStaleInput() {
        val sent = mutableListOf<String>()
        val keys = mutableListOf<Int>()
        val target = object : TerminalInputTarget {
            override fun text(text: String): Boolean { sent += text; return true }
            override fun key(code: Int, modifiers: Int, action: Int, text: String, unshifted: Int): Boolean {
                keys += code; return true
            }
        }
        var keyboardRequest by mutableIntStateOf(0)
        var allowInput by mutableStateOf(true)
        lateinit var root: View
        compose.setContent {
            root = LocalView.current
            MaterialTheme {
                DesktopOutputSurface("pc-pane", "Legacy terminal output", 16, true, {},
                    Modifier.size(300.dp, 240.dp).testTag("output"),
                    inputTarget = target.takeIf { allowInput }, keyboardRequest = keyboardRequest)
            }
        }
        compose.waitForIdle()
        val view = compose.runOnIdle {
            checkNotNull(findInputView(root)) { "Legacy text must retain the native input target" }
                .also { it.clearFocus() }
        }
        compose.runOnIdle { keyboardRequest++ }
        compose.waitForIdle()
        val ime = compose.runOnIdle {
            assertTrue("keyboard action focuses the actual native editor", view.hasFocus())
            checkNotNull(view.onCreateInputConnection(EditorInfo()))
        }
        compose.runOnIdle {
            assertTrue(ime.setComposingText("中", 1))
            assertTrue(sent.isEmpty())
            assertTrue(ime.commitText("中文😀", 1))
            assertEquals(listOf("中文😀"), sent)
            view.clearFocus()
        }
        compose.onNodeWithText("Legacy terminal output").performTouchInput { click() }
        compose.runOnIdle {
            assertTrue("a tap on the output opens input", view.hasFocus())
            assertTrue("focusing the editor must not submit an empty command", keys.isEmpty())
            assertEquals(listOf("中文😀"), sent)
        }

        compose.runOnIdle { allowInput = false }
        compose.waitForIdle()
        compose.onNodeWithText("Legacy terminal output").performTouchInput { click() }
        compose.runOnIdle {
            assertFalse(view.hasFocus())
            assertNull(view.onCreateInputConnection(EditorInfo()))
            assertFalse(ime.commitText("stale", 1))
            assertEquals(listOf("中文😀"), sent)
        }
    }

    private fun findInputView(view: View): TerminalSnapshotView? {
        if (view is TerminalSnapshotView) return view
        if (view is ViewGroup) for (i in 0 until view.childCount) {
            findInputView(view.getChildAt(i))?.let { return it }
        }
        return null
    }

    @Test fun legacyZoomScalesLinePitchWithGlyphWidth() {
        val output = "MMMMMMMM\nMMMMMMMM\nMMMMMMMM"
        compose.setContent {
            var size by remember { mutableIntStateOf(16) }
            MaterialTheme {
                DesktopOutputSurface("pc-pane", output, size, true, { size = it },
                    Modifier.size(300.dp, 300.dp).testTag("output"))
            }
        }
        fun layout(): TextLayoutResult {
            val result = mutableListOf<TextLayoutResult>()
            compose.onNodeWithText(output).performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(result) }
            return result.single()
        }
        val before = layout()
        zoom(70f)
        val after = layout()
        val widthRatio = after.getBoundingBox(7).right / before.getBoundingBox(7).right
        val rowRatio = (after.getLineBaseline(1) - after.getLineBaseline(0)) /
            (before.getLineBaseline(1) - before.getLineBaseline(0))
        assertTrue("pinch must enlarge glyphs: ratio=$widthRatio, before=${before.layoutInput.style.fontSize}, " +
            "after=${after.layoutInput.style.fontSize}", widthRatio > 1.1f)
        val fontRatio = after.layoutInput.style.fontSize.value / before.layoutInput.style.fontSize.value
        // 原生字号落到物理像素时会取整；按一像素容差核对行距，不以取整后的单字宽充当比例。
        assertEquals("line pitch must scale with glyphs, not stay at the theme's fixed line height",
            fontRatio, rowRatio, 1f / (before.getLineBaseline(1) - before.getLineBaseline(0)))
    }

    @Test fun successivePinchesStillWorkAfterTheFirstFontPreferenceIsSaved() {
        val sizes = mutableListOf<Int>()
        compose.setContent {
            var size by remember { mutableIntStateOf(16) }
            MaterialTheme {
                DesktopOutputSurface("pc-pane", "Terminal output", size, true, {
                    sizes += it
                    size = it
                }, Modifier.size(300.dp, 240.dp).testTag("output"))
            }
        }
        zoom(60f)
        assertEquals(1, sizes.size)
        assertTrue(sizes[0] > 16)
        zoom(70f)
        assertEquals(2, sizes.size)
        assertTrue(sizes[1] > sizes[0])
        assertTrue(sizes[1] <= 32)
    }

    @Test fun pinchShowsCenteredSizeTicksAndDismissesAfterReleaseWithoutTakingLayoutSpace() {
        lateinit var root: View
        compose.setContent {
            root = LocalView.current
            var size by remember { mutableIntStateOf(16) }
            MaterialTheme {
                DesktopOutputSurface("pc-pane", "First line\nSecond line\nThird line", size, true, { size = it },
                    Modifier.size(300.dp, 300.dp).testTag("output"))
            }
        }
        val bounds = compose.onNodeWithTag("output").fetchSemanticsNode().boundsInRoot
        compose.mainClock.autoAdvance = false
        compose.onNodeWithTag("output").performTouchInput {
            down(0, center + Offset(-50f, 0f))
            down(1, center + Offset(50f, 0f))
            moveTo(0, center + Offset(-80f, 0f))
            moveTo(1, center + Offset(80f, 0f))
        }
        compose.waitForIdle()
        compose.mainClock.advanceTimeBy(150)
        val indicator = compose.onNodeWithTag("terminal-zoom-feedback").fetchSemanticsNode().boundsInRoot
        compose.onNodeWithTag("terminal-zoom-feedback").assertIsDisplayed()
        assertEquals(bounds.center.x, indicator.center.x, 1f)
        assertEquals(bounds.center.y, indicator.center.y, 1f)
        assertEquals(bounds, compose.onNodeWithTag("output").fetchSemanticsNode().boundsInRoot)
        compose.runOnIdle { assertEquals(HapticFeedbackConstants.CLOCK_TICK, shadowOf(root).lastHapticFeedbackPerformed()) }
        compose.onNodeWithTag("output").performTouchInput { up(0); up(1) }
        compose.mainClock.advanceTimeBy(150)
        compose.onNodeWithTag("terminal-zoom-feedback").assertIsDisplayed()
        compose.mainClock.advanceTimeBy(900)
        compose.onNodeWithTag("terminal-zoom-feedback").assertDoesNotExist()
        compose.mainClock.autoAdvance = true
    }

    private fun zoom(distance: Float) {
        compose.onNodeWithTag("output").performTouchInput {
            down(0, center + Offset(-50f, 0f))
            down(1, center + Offset(50f, 0f))
            moveTo(0, center + Offset(-distance, 0f))
            moveTo(1, center + Offset(distance, 0f))
            up(0)
            up(1)
        }
        compose.waitForIdle()
    }

    // Robolectric 的 API 28 原生 Magnifier 没有 SurfaceControl；此处只验证滚动/IME 归属，
    // 带放大镜的长按选区另在 API 28 雷电的真实窗口中验证。
    @Config(sdk = [27])
    @Test fun singleFingerScrollingDoesNotChangeFontSize() {
        var saves = 0
        lateinit var root: View
        val target = object : TerminalInputTarget {
            override fun text(text: String) = true
            override fun key(code: Int, modifiers: Int, action: Int, text: String, unshifted: Int) = true
        }
        compose.setContent {
            root = LocalView.current
            MaterialTheme {
                DesktopOutputSurface("pc-pane", (1..80).joinToString("\n") { "Output line $it" }, 16,
                    true, { saves++ }, Modifier.size(300.dp, 240.dp).testTag("output"), inputTarget = target)
            }
        }
        compose.runOnIdle { checkNotNull(findInputView(root)).clearFocus() }
        compose.onNodeWithTag("output").performTouchInput { swipeUp() }
        compose.waitForIdle()
        assertEquals(0, saves)
        val scroll = compose.onNode(SemanticsMatcher.keyIsDefined(SemanticsProperties.VerticalScrollAxisRange))
            .fetchSemanticsNode().config[SemanticsProperties.VerticalScrollAxisRange]
        assertTrue(scroll.value() > 0f)
        compose.runOnIdle { assertFalse("scroll must not open the IME", checkNotNull(findInputView(root)).hasFocus()) }
        compose.onNodeWithTag("output").performTouchInput { longClick() }
        compose.runOnIdle { assertFalse("long-press selection must not open the IME", checkNotNull(findInputView(root)).hasFocus()) }
    }

    @Test fun legacyOutputStartsAtTheTailAndPreservesAnExplicitScrollUntilLatestIsTapped() {
        val original = "\n\n\n" + (1..80).joinToString("\n") { "Output line $it" }
        var output by mutableStateOf(original)
        compose.setContent { MaterialTheme {
            DesktopOutputSurface("pc-pane", output, 16, true, {},
                Modifier.size(300.dp, 240.dp).testTag("output"))
        } }
        fun range() = compose.onNode(SemanticsMatcher.keyIsDefined(SemanticsProperties.VerticalScrollAxisRange))
            .fetchSemanticsNode().config[SemanticsProperties.VerticalScrollAxisRange]
        compose.waitForIdle()
        compose.onNodeWithText(original).assertExists()
        assertEquals(range().maxValue(), range().value(), 1f)
        compose.onNodeWithTag("output").performTouchInput { swipeDown() }
        compose.waitForIdle()
        val reading = range().value()
        assertTrue(reading < range().maxValue())
        compose.runOnIdle { output += "\nNew output" }
        compose.waitForIdle()
        assertEquals(reading, range().value(), 1f)
        compose.onNodeWithText("Latest output ↓").performClick()
        compose.waitForIdle()
        assertEquals(range().maxValue(), range().value(), 1f)
    }
}
