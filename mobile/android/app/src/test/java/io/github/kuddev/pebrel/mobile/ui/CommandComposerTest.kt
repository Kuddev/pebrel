package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.*
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.platform.testTag
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.width
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
    @Test fun scrollbackDefaultsToOneThousandAndPersistsWithinTheDeviceLimit() {
        val context = ApplicationProvider.getApplicationContext<PebrelApplication>()
        val preferences = DisplayPreferences(context)
        assertEquals(1000, preferences.state.value.scrollbackLines)
        preferences.update { it.copy(scrollbackLines = 50_000) }
        assertEquals(preferences.maxScrollbackLines, preferences.state.value.scrollbackLines)
        assertEquals(preferences.state.value.scrollbackLines, DisplayPreferences(context).state.value.scrollbackLines)
        preferences.update { it.copy(scrollbackLines = 1000) }
    }

    @Test fun fileSymbolsUseDedicatedOutlineResources() {
        val context = ApplicationProvider.getApplicationContext<PebrelApplication>()
        assertNotEquals(fileSymbol("README.md", false), fileSymbol("README.unknown", false))
        assertEquals(fileSymbol("readme.md", false), fileSymbol("/项目/README.MD", false))
        assertNotEquals(fileSymbol("src", true), fileSymbol("src", false))
        assertEquals(R.drawable.ic_file_md, fileSymbol("README", false))
        assertEquals(R.drawable.ic_file_docker, fileSymbol("Dockerfile.dev", false))
        assertEquals(R.drawable.ic_file_kt, fileSymbol("MainActivity.kt", false))
        assertEquals(R.drawable.ic_file_go, fileSymbol("server.go", false))
        assertEquals(R.drawable.ic_file_powershell, fileSymbol("build.ps1", false))
        val types = listOf("README.md", "main.rs", "script.py", "config.json", "config.toml", "config.yaml", "notes.txt", "image.png", "archive.zip", "manual.pdf")
        assertEquals(types.size, types.map { fileSymbol(it, false) }.distinct().size)
        for (path in types) {
            assertTrue(path, context.resources.getDrawable(fileSymbol(path, false), context.theme).intrinsicWidth > 0)
        }
    }

    @Test fun keyAuthenticationCanBeSelectedAndRequiresAKeyDocument() {
        val context = ApplicationProvider.getApplicationContext<PebrelApplication>()
        compose.setContent {
            MaterialTheme { HostForm(HostProfile("key-form", "Key host", "127.0.0.1", 22, "test"), {}, false, false, {}, onSave = { _, _, _, _ -> }) }
        }
        assertTrue(compose.onNodeWithTag("ssh-session-mode").fetchSemanticsNode().boundsInRoot.width <= 240 * context.resources.displayMetrics.density)
        assertTrue(compose.onNodeWithTag("ssh-auth-mode").fetchSemanticsNode().boundsInRoot.width <= 220 * context.resources.displayMetrics.density)
        compose.onNodeWithText(context.getString(R.string.auth_key)).performScrollTo().assertIsEnabled().performClick()
        compose.onNodeWithText(context.getString(R.string.ssh_choose_key)).performScrollTo().assertIsDisplayed().assertIsEnabled()
        compose.onNodeWithText(context.getString(R.string.save_connect)).performScrollTo().assertIsNotEnabled()
        compose.onNodeWithText(context.getString(R.string.auth_auto)).performScrollTo().performClick()
        compose.onNodeWithText(context.getString(R.string.save_connect)).performScrollTo().assertIsEnabled()
    }

    @Test fun savedKeyFormUsesPassphraseLabelsInsteadOfPasswordLabels() {
        val context = ApplicationProvider.getApplicationContext<PebrelApplication>()
        compose.setContent {
            MaterialTheme { HostForm(HostProfile("saved-key", "Key host", "127.0.0.1", 22, "test",
                keyUri = "content://fixture/key", keyName = "encrypted-key"), {}, true, false, {}, onSave = { _, _, _, _ -> }) }
        }
        compose.onNodeWithText(context.getString(R.string.ssh_clear_saved_passphrase)).performScrollTo().assertIsDisplayed()
        compose.onNodeWithText(context.getString(R.string.ssh_passphrase_saved_hint)).performScrollTo().assertIsDisplayed()
        compose.onNodeWithText(context.getString(R.string.clear_saved_password)).assertDoesNotExist()
        compose.onNodeWithText(context.getString(R.string.password_saved_hint)).assertDoesNotExist()
    }

    @Test fun compactSegmentsKeepLargeEnglishLabelsAndFortyEightDpTargets() {
        var chosen by mutableStateOf("production")
        var scale by mutableFloatStateOf(1f)
        compose.setContent {
            val density = androidx.compose.ui.platform.LocalDensity.current
            CompositionLocalProvider(androidx.compose.ui.platform.LocalDensity provides androidx.compose.ui.unit.Density(density.density, scale)) {
                MaterialTheme {
                    Box(Modifier.width(320.dp)) {
                        ConnectionSegments(listOf("production" to "Production", "development" to "Development"),
                            chosen, { chosen = it }, Modifier.testTag("compact-segments"), compact = true)
                    }
                }
            }
        }
        for (fontScale in listOf(1f, 1.5f)) {
            compose.runOnIdle { scale = fontScale }
            val density = ApplicationProvider.getApplicationContext<PebrelApplication>().resources.displayMetrics.density
            assertTrue(compose.onNodeWithTag("compact-segments").fetchSemanticsNode().boundsInRoot.width <= 280 * density)
            for (label in listOf("Production", "Development")) {
                compose.onNode(hasText(label) and hasClickAction()).assertWidthIsAtLeast(48.dp).assertHeightIsAtLeast(48.dp).performClick()
            }
            compose.runOnIdle { assertEquals("development", chosen) }
        }
    }

    @Test fun defaultComposerExposesShiftTabInTheNarrowShortcutMenu() {
        val context = ApplicationProvider.getApplicationContext<PebrelApplication>()
        val repository = SessionRepository(context)
        val sent = mutableListOf<String>()
        var allowed by mutableStateOf(true)
        compose.setContent { MaterialTheme {
            Box(Modifier.width(240.dp)) {
                CommandComposer("backtab", repository, allowed, true, null, { sent += it }) { true }
            }
        } }
        compose.onNodeWithContentDescription(context.getString(R.string.terminal_shortcuts_more)).performClick()
        compose.onAllNodesWithText("Shift+Tab").onLast().assertIsDisplayed().performClick()
        assertEquals(listOf("Shift+Tab"), sent)
        compose.runOnIdle { allowed = false }
        compose.onNodeWithContentDescription(context.getString(R.string.terminal_shortcuts_more)).performClick()
        compose.onAllNodesWithText("Shift+Tab").onLast().assertIsNotEnabled()
        assertEquals(listOf("Shift+Tab"), sent)
    }

    @Test fun compactShortcutOverflowProvidesATappableMenuAndRespectsInputPermission() {
        val context = ApplicationProvider.getApplicationContext<PebrelApplication>()
        val sent = mutableListOf<String>()
        var allowed by mutableStateOf(true)
        compose.setContent { MaterialTheme {
            Box(Modifier.width(240.dp)) {
                ComposerToolbar(onEdit = {}, keyboardVisible = false, keyboardEnabled = true,
                    onImeToggle = {}, enabled = allowed,
                    shortcuts = listOf("Ctrl+C", "Esc", "Tab", "←", "→", "↑", "↓"), onKey = { sent += it })
            }
        } }
        compose.onNodeWithContentDescription(context.getString(R.string.terminal_shortcuts_more)).performClick()
        compose.onAllNodesWithText("↓").onLast().performClick()
        assertEquals(listOf("↓"), sent)
        compose.runOnIdle { allowed = false }
        compose.onNodeWithContentDescription(context.getString(R.string.terminal_shortcuts_more)).performClick()
        compose.onAllNodesWithText("↓").onLast().assertIsNotEnabled()
        assertEquals(listOf("↓"), sent)
    }
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
    @Test fun readOnlyPaneExplainsLiveAuthorizationWithoutPairingAgain() {
        val context = ApplicationProvider.getApplicationContext<PebrelApplication>()
        val repository = SessionRepository(context)
        val allowInput = mutableStateOf(false)
        compose.setContent { MaterialTheme { DesktopTerminalScreen(
            DesktopWorkspace("pc", HostProfile("host", "PC", "192.0.2.1", 22, "root"), status = "ready", allowInput = allowInput.value),
            DesktopPane(1, 1, "shell", "", "", "idle", 0), repository, {}, {}) } }
        compose.onNodeWithText(context.getString(R.string.composer_pc_read_only_short)).assertExists()
        compose.onNodeWithText(context.getString(R.string.composer_pc_enable_input)).performClick()
        compose.onNodeWithText(context.getString(R.string.composer_pc_read_only)).assertExists()
        compose.onNodeWithText(context.getString(R.string.close)).performClick()
        compose.onNodeWithText(context.getString(R.string.composer_pc_read_only)).assertDoesNotExist()
        compose.onNodeWithText(context.getString(R.string.composer_pc_enable_input)).performClick()
        compose.runOnIdle { allowInput.value = true }
        compose.onNodeWithText(context.getString(R.string.composer_pc_read_only)).assertDoesNotExist()
        compose.onNodeWithText(context.getString(R.string.composer_pc_read_only_short)).assertDoesNotExist()
    }

    @Test fun terminalHeaderUsesTheTabLabelAndAnAccessibleGitIconWithFullPathInDetails() {
        val context = ApplicationProvider.getApplicationContext<PebrelApplication>()
        val repository = SessionRepository(context)
        val path = "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"
        val cwd = "C:\\workspace\\project"
        val ready = mutableStateOf(true)
        var opened = 0
        compose.setContent { MaterialTheme { Column { DesktopTerminalScreen(
            DesktopWorkspace("pc", HostProfile("host", "PC", "192.0.2.1", 22, "root"), status = if (ready.value) "ready" else "disconnected"),
            DesktopPane(1, 1, path, cwd, "", "idle", 0, tabLabel = "work"), repository, {}, {}, onGit = { opened++ }) } } }
        compose.onNodeWithText("work").assertIsDisplayed()
        compose.onNodeWithText(path).assertDoesNotExist()
        compose.onNodeWithText(context.getString(R.string.git_title)).assertDoesNotExist()
        compose.onNodeWithContentDescription(context.getString(R.string.git_title))
            // 右上角使用已确认的紧凑排列；高度仍保留 48dp，普通按钮不随之缩小。
            .assertHeightIsAtLeast(48.dp).assertWidthIsEqualTo(36.dp).assertIsEnabled().performClick()
        compose.runOnIdle { assertEquals(1, opened); ready.value = false }
        compose.onNodeWithContentDescription(context.getString(R.string.git_title)).assertIsNotEnabled()
        compose.onNodeWithContentDescription(context.getString(R.string.more_actions)).performClick()
        compose.onNodeWithText(context.getString(R.string.terminal_details)).performClick()
        compose.onNodeWithText(path).assertIsDisplayed()
        compose.onNodeWithText(cwd).assertIsDisplayed()
        compose.onNodeWithText(context.getString(R.string.close)).performClick()
        compose.onNodeWithText(path).assertDoesNotExist()
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
        compose.onNodeWithText(context.getString(R.string.service_advanced)).performScrollTo().performClick()
        compose.onNodeWithContentDescription(context.getString(R.string.service_port)).assertExists()
        compose.onNodeWithContentDescription(context.getString(R.string.service_address)).assertExists()
        compose.onNodeWithText(context.getString(R.string.service_manual_commands)).performScrollTo().performClick()
        compose.onNodeWithText("sh install.sh 'SERVER_IP' 443\n/opt/pebrel-relay/pebrel-relay service-status").assertExists()
        compose.onNodeWithText(context.getString(R.string.service_manual_download)).performScrollTo().performClick()
        val opened = org.robolectric.Shadows.shadowOf(context).nextStartedActivity
        assertEquals(android.content.Intent.ACTION_VIEW, opened.action)
        assertEquals("https://github.com/Kuddev/pebrel/releases", opened.dataString)
    }

    @Test fun servicePageAllowsAnUnencryptedKeyAndSeparatesItsPassphraseFromAPassword() {
        val context = ApplicationProvider.getApplicationContext<PebrelApplication>()
        val repository = SessionRepository(context)
        val keyHost = HostProfile("relay-key", "Key host", "192.0.2.1", user = "root",
            keyUri = "content://fixture/private-key", keyName = "private-key")
        repository.saveHost(keyHost)
        compose.setContent { MaterialTheme { RelayDeploymentFlow(repository) {} } }
        compose.onNodeWithContentDescription(context.getString(R.string.ssh_key_passphrase)).assertExists()
        compose.onNodeWithText(context.getString(R.string.ssh_key_passphrase_hint)).assertExists()
        compose.onNodeWithText(context.getString(R.string.service_install)).assertIsEnabled()
        compose.onNodeWithText(context.getString(R.string.service_check)).assertIsEnabled()
        compose.onNodeWithContentDescription(context.getString(R.string.ssh_key_passphrase))
            .performTextInput("private-key-passphrase")
        compose.onNodeWithText(context.getString(R.string.service_install)).assertIsEnabled()
        compose.runOnIdle { repository.saveHost(keyHost.copy(keyUri = "", keyName = "")) }
        compose.onNodeWithContentDescription(context.getString(R.string.credential_password))
            .performTextClearance()
        compose.onNodeWithText(context.getString(R.string.service_install)).assertIsNotEnabled()
    }

    @Test fun serviceSetupCanOpenTheSharedKeyPickerAndCancelWithoutLosingItsHost() {
        val context = ApplicationProvider.getApplicationContext<PebrelApplication>()
        val repository = SessionRepository(context)
        repository.saveHost(HostProfile("existing-relay", "Existing relay", "192.0.2.1", user = "root"))
        compose.setContent { MaterialTheme { RelayDeploymentFlow(repository) {} } }
        compose.onNodeWithText(context.getString(R.string.service_add_ssh_host)).performClick()
        compose.onNodeWithText(context.getString(R.string.auth_key)).performScrollTo().performClick()
        compose.onNodeWithText(context.getString(R.string.ssh_choose_key)).performScrollTo().assertIsDisplayed()
        compose.onNodeWithText(context.getString(R.string.save_connect)).assertDoesNotExist()
        compose.onAllNodesWithContentDescription(context.getString(R.string.close)).onLast().performScrollTo().performClick()
        compose.onNodeWithText("Existing relay").assertExists()
        compose.onNodeWithText(context.getString(R.string.service_install)).assertExists()
        assertEquals(1, repository.hosts.value.size)
        assertTrue(repository.sessions.value.isEmpty())
    }

    @Test fun serviceSetupSavesAndSelectsTheNewHostWithoutOpeningATerminal() {
        val context = ApplicationProvider.getApplicationContext<PebrelApplication>()
        val repository = SessionRepository(context)
        repository.saveHost(HostProfile("existing-relay", "Existing relay", "192.0.2.1", user = "root"))
        compose.setContent { MaterialTheme { RelayDeploymentFlow(repository) {} } }
        compose.onNodeWithText(context.getString(R.string.service_add_ssh_host)).performClick()
        compose.onNodeWithContentDescription(context.getString(R.string.host_name)).performTextInput("New relay")
        compose.onNodeWithContentDescription(context.getString(R.string.host_address)).performTextInput("192.0.2.2")
        compose.onNodeWithText(context.getString(R.string.save)).performScrollTo().assertIsEnabled().performClick()
        compose.waitUntil(10_000) {
            // 凭据事务在 IO 完成后投递 Android 主队列，需推进它而不只推进 Compose 时钟。
            org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
            repository.hosts.value.size == 2 || repository.error.value != null
        }
        assertNull("Host persistence must complete before returning to relay setup", repository.error.value)
        compose.onNodeWithText("New relay").assertExists()
        compose.onNodeWithText("Existing relay").assertDoesNotExist()
        compose.onNodeWithText(context.getString(R.string.service_install)).assertExists()
        assertTrue(repository.sessions.value.isEmpty())
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
