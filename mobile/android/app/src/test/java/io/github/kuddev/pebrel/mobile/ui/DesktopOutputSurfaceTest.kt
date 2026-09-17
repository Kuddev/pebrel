package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.unit.dp
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class DesktopOutputSurfaceTest {
    @get:Rule val compose = createComposeRule()

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
        fun zoom(distance: Float) {
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
        zoom(60f)
        assertEquals(1, sizes.size)
        assertTrue(sizes[0] > 16)
        zoom(70f)
        assertEquals(2, sizes.size)
        assertTrue(sizes[1] > sizes[0])
        assertTrue(sizes[1] <= 32)
    }

    @Test fun singleFingerScrollingDoesNotChangeFontSize() {
        var saves = 0
        compose.setContent {
            MaterialTheme {
                DesktopOutputSurface("pc-pane", (1..80).joinToString("\n") { "Output line $it" }, 16,
                    true, { saves++ }, Modifier.size(300.dp, 240.dp).testTag("output"))
            }
        }
        compose.onNodeWithTag("output").performTouchInput { swipeUp() }
        compose.waitForIdle()
        assertEquals(0, saves)
        val scroll = compose.onNode(SemanticsMatcher.keyIsDefined(SemanticsProperties.VerticalScrollAxisRange))
            .fetchSemanticsNode().config[SemanticsProperties.VerticalScrollAxisRange]
        assertTrue(scroll.value() > 0f)
    }
}
