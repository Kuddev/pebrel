package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.*
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.foundation.layout.Box
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
    @Test fun installFailureShowsAllFourNumberedStepsAndActualUploadCount() {
        val context = ApplicationProvider.getApplicationContext<PebrelApplication>()
        val progress = io.github.kuddev.pebrel.mobile.connection.RelayInstallProgress()
            .advance(io.github.kuddev.pebrel.mobile.connection.RelayServiceProgress("uploading", 32768, 65536))
            .copy(failed = true)
        compose.setContent {
            renderedView = LocalView.current
            MaterialTheme { Box(Modifier.testTag("relay-install-steps")) { RelayInstallSteps(progress, R.string.ssh_error_timeout) } }
        }
        val titles = listOf(R.string.service_step_connect, R.string.service_step_check, R.string.service_step_upload, R.string.service_step_start)
        val states = listOf(R.string.service_step_done, R.string.service_step_done, R.string.service_operation_failed, R.string.service_step_waiting)
        titles.forEachIndexed { i, title ->
            compose.onNodeWithText(context.getString(R.string.service_step_row, i + 1, context.getString(title), context.getString(states[i]))).assertIsDisplayed()
        }
        compose.onNodeWithText(context.getString(R.string.service_upload_bytes, 32, 64)).assertIsDisplayed()
        compose.onNodeWithText(context.getString(R.string.ssh_error_timeout)).assertIsDisplayed()
        saveSurface("relay-install-steps")
    }
    @Test fun readOnlyPaneExplainsAuthorizationAndOffersPairingWithoutSending() {
        val context = ApplicationProvider.getApplicationContext<PebrelApplication>()
        val repository = SessionRepository(context)
        var scanned = false
        compose.setContent { MaterialTheme { DesktopTerminalScreen(
            DesktopWorkspace("pc", HostProfile("host", "PC", "192.0.2.1", 22, "root"), status = "ready", allowInput = false),
            DesktopPane(1, 1, "shell", "", "", "idle", 0), repository, {}, {}, { scanned = true }) } }
        compose.onNodeWithText(context.getString(R.string.composer_pc_read_only_short)).assertExists()
        compose.onNodeWithText(context.getString(R.string.composer_pc_enable_input)).performClick()
        compose.onNodeWithText(context.getString(R.string.composer_pc_read_only)).assertExists()
        compose.onNodeWithText(context.getString(R.string.composer_pc_scan_again)).performClick()
        compose.runOnIdle { assertTrue(scanned) }
    }

    @Test fun terminalChromeUsesRemoteDarkAndLightColorsAndFailureEndsProgress() {
        val fallback = androidx.compose.material3.lightColorScheme()
        for ((bg, fg) in listOf(0xff2e3440.toInt() to 0xffeceff4.toInt(), 0xfffcfbf9.toInt() to 0xff222222.toInt())) {
            val frame = io.github.kuddev.pebrel.terminal.TerminalFrame(emptyArray(), intArrayOf(1, 1, 0, 0, 0, bg, fg, 2, fg, 0xffbf616a.toInt()))
            val scheme = desktopTerminalColors(frame, fallback)
            assertEquals(androidx.compose.ui.graphics.Color(bg), scheme.background)
            assertEquals(scheme.background, scheme.surface)
            assertEquals(androidx.compose.ui.graphics.Color(fg), scheme.onSurface)
        }
        assertEquals(fallback, desktopTerminalColors(null, fallback))
        assertEquals(R.string.service_operation_failed, serviceStageText("failed"))
        assertEquals(R.string.service_openrc_error, serviceErrorText(io.github.kuddev.pebrel.mobile.connection.RelayServiceFailure("openrc_supervisor_required")))
    }
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
        compose.onNodeWithText(context.getString(R.string.service_manual_commands)).performScrollTo().performClick()
        compose.onNodeWithText("sh install.sh 'SERVER_IP' 443\n/opt/pebrel-relay/pebrel-relay service-status").assertExists()
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
