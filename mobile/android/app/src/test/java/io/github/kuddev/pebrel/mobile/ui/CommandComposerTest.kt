package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.*
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.platform.LocalView
import android.graphics.Bitmap
import android.graphics.Canvas
import android.view.View
import java.io.File
import androidx.test.core.app.ApplicationProvider
import io.github.kuddev.pebrel.mobile.PebrelApplication
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.session.SessionRepository
import io.github.kuddev.pebrel.mobile.session.DisplayPreferences
import io.github.kuddev.pebrel.mobile.session.DesktopWorkspace
import io.github.kuddev.pebrel.mobile.connection.HostProfile
import io.github.kuddev.pebrel.mobile.connection.DesktopPane
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class CommandComposerTest {
    @get:Rule val compose = createComposeRule()
    private var renderedView: View? = null

    @Test fun oneSlotSwitchesModesWithoutLosingDraftOrKeepingAnEmptyToolbar() {
        val context = ApplicationProvider.getApplicationContext<PebrelApplication>()
        val repository = SessionRepository(context)
        compose.setContent {
            renderedView = LocalView.current
            var direct by remember { mutableStateOf(true) }
            MaterialTheme {
                CommandComposer("test-session", repository, true, direct, { direct = it }, {}, onKeyboard = {}) { true }
            }
        }
        compose.onNodeWithTag("composer-direct").assertExists()
        compose.onNodeWithTag("composer-editor").assertDoesNotExist()
        compose.onAllNodes(hasSetTextAction()).assertCountEquals(0)
        saveSurface("composer-direct")
        compose.onNodeWithContentDescription(context.getString(R.string.composer_mode_edit)).performClick()
        compose.onNodeWithTag("composer-direct").assertDoesNotExist()
        compose.onNodeWithTag("composer-editor").assertExists()
        compose.onAllNodes(hasSetTextAction()).assertCountEquals(1)
        saveSurface("composer-editor")
        compose.onNode(hasSetTextAction()).performTextInput("echo preserved")
        compose.onNodeWithContentDescription(context.getString(R.string.composer_mode_direct)).performClick()
        compose.onNodeWithTag("composer-editor").assertDoesNotExist()
        compose.onNodeWithTag("composer-direct").assertExists()
        compose.onNodeWithContentDescription(context.getString(R.string.composer_mode_edit)).performClick()
        compose.onNode(hasSetTextAction()).assertTextEquals("echo preserved")
        compose.onNodeWithTag("composer-direct").assertDoesNotExist()
    }

    @Test fun desktopScreenStartsCompactAndCanSwitchToOneEditor() {
        val context = ApplicationProvider.getApplicationContext<PebrelApplication>()
        val repository = SessionRepository(context)
        compose.setContent {
            MaterialTheme { DesktopTerminalScreen(
                DesktopWorkspace("pc", HostProfile("host", "PC", "192.0.2.1", 22, "user")),
                DesktopPane(1, 1, "shell", "", "", "idle", 0), repository, {}, {}) }
        }
        compose.onNodeWithTag("composer-editor").assertDoesNotExist()
        compose.onNodeWithTag("composer-direct").assertExists()
        compose.onNodeWithContentDescription(context.getString(R.string.composer_mode_edit)).performClick()
        compose.onNodeWithTag("composer-direct").assertDoesNotExist()
        compose.onAllNodes(hasSetTextAction()).assertCountEquals(1)
    }

    @Test fun oldComposerFirstPreferenceMigratesOnceAndLaterChoiceIsPreserved() {
        val context = ApplicationProvider.getApplicationContext<PebrelApplication>()
        val stored = context.getSharedPreferences("terminal_display", 0)
        stored.edit().remove("compact_input_default_v1").putBoolean("direct_input", false).commit()
        val display = DisplayPreferences(context)
        assertTrue(display.state.value.directInput)
        display.update { it.copy(directInput = false) }
        assertFalse(DisplayPreferences(context).state.value.directInput)
        display.update { it.copy(directInput = true) }
    }

    @Test fun servicePageKeepsNetworkInternalsUnderAdvanced() {
        val context = ApplicationProvider.getApplicationContext<PebrelApplication>()
        val repository = SessionRepository(context)
        compose.setContent { MaterialTheme { RelayDeploymentFlow(repository) {} } }
        compose.onNodeWithText(context.getString(R.string.service_install)).assertExists().assertIsNotEnabled()
        compose.onNodeWithText(context.getString(R.string.deploy_domain)).assertDoesNotExist()
        compose.onNodeWithText(context.getString(R.string.deploy_http_port)).assertDoesNotExist()
        compose.onNodeWithText(context.getString(R.string.service_port)).assertDoesNotExist()
        compose.onNodeWithText(context.getString(R.string.service_advanced)).performClick()
        compose.onNodeWithContentDescription(context.getString(R.string.service_port)).assertExists()
        compose.onNodeWithContentDescription(context.getString(R.string.service_address)).assertExists()
    }

    private fun saveSurface(tag: String) {
        val file = File("build/reports/composer/$tag.png")
        check(file.parentFile!!.isDirectory || file.parentFile!!.mkdirs())
        val bounds = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot
        compose.runOnIdle {
            // PixelCopy needs an emulator surface; use the actual Compose view's
            // native Canvas in Robolectric, preserving the production layout.
            val bitmap = Bitmap.createBitmap(bounds.width.toInt(), bounds.height.toInt(), Bitmap.Config.ARGB_8888)
            val canvas = Canvas(bitmap)
            canvas.translate(-bounds.left, -bounds.top)
            checkNotNull(renderedView).draw(canvas)
            file.outputStream().use { check(bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)) }
            bitmap.recycle()
        }
    }
}
