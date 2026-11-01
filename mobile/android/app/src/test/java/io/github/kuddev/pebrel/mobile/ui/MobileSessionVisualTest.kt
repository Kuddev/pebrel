package io.github.kuddev.pebrel.mobile.ui

import android.graphics.Bitmap
import android.graphics.Canvas
import android.view.View
import androidx.compose.foundation.layout.*
import androidx.compose.material3.Surface
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.test.core.app.ApplicationProvider
import io.github.kuddev.pebrel.mobile.PebrelApplication
import io.github.kuddev.pebrel.mobile.connection.DesktopPane
import io.github.kuddev.pebrel.mobile.connection.HostProfile
import io.github.kuddev.pebrel.mobile.session.DesktopWorkspace
import io.github.kuddev.pebrel.mobile.session.LocalSession
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.shadows.ShadowDialog
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], qualifiers = "w390dp-h844dp-mdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class MobileSessionVisualTest {
    @get:Rule val compose = createComposeRule()
    private var view: View? = null
    private val terminal = galleryTerminalFixture()

    @After fun closeTerminal() { terminal.finishIfRunning() }

    @Test fun overviewKeepsPreviewCardsAndThemeHierarchy() {
        val app = ApplicationProvider.getApplicationContext<PebrelApplication>()
        val pane = DesktopPane(1, 2, "Review mobile conversation", "/workspace/pebrel", "Claude Code", "running", 1)
        val desktop = DesktopWorkspace("test-pc", HostProfile("test-host", "Development Mac", "192.0.2.1", 22, "dev"),
            panes = listOf(pane, pane.copy(id = 3, title = "Check SSH connection", state = "waiting_input")),
            status = "ready", transport = "LAN", hasConnected = true)
        var opened: Long? = null
        compose.setContent {
            view = LocalView.current
            PebrelTheme { Surface(Modifier.fillMaxSize().testTag("mobile-overview")) { Column {
                HomeHeader({}, {})
                HomeScreen(listOf(LocalSession("local-preview", "Build preview", "Local", terminal, "ready")),
                    emptyList(), listOf(desktop), emptyList(),
                    {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, onPane = { _, selected -> opened = selected.id })
            } } }
        }
        for (theme in listOf("MintLight", "Paper", "Nord")) {
            compose.runOnIdle { app.sessions.selectTheme(theme) }
            compose.onNodeWithText(pane.title).assertIsDisplayed().performClick()
            assertEquals(2L, opened)
            compose.onNodeWithText("$ cargo test\n24 tests passed\n~/workspace $").assertIsDisplayed()
            save("overview-gallery-${theme.lowercase()}", "mobile-overview")
        }
    }

    @Test fun keyLoginRemainsReachableWithTheLightTheme() {
        val app = ApplicationProvider.getApplicationContext<PebrelApplication>()
        app.sessions.selectTheme("MintLight")
        compose.setContent {
            view = LocalView.current
            PebrelTheme { Surface(Modifier.fillMaxSize().testTag("ssh-key-form")) {
                HostForm(HostProfile("key-preview", "Development server", "192.0.2.1", 22, "dev"),
                    {}, false, false, {}, onSave = { _, _, _, _ -> })
            } }
        }
        compose.onNodeWithText(app.getString(io.github.kuddev.pebrel.mobile.R.string.auth_key))
            .performScrollTo().performClick()
        compose.onNodeWithText(app.getString(io.github.kuddev.pebrel.mobile.R.string.ssh_choose_key))
            .performScrollTo().assertIsDisplayed().assertIsEnabled()
        compose.runOnIdle {
            val dialogView = checkNotNull(ShadowDialog.getLatestDialog().window).decorView
            val bitmap = Bitmap.createBitmap(dialogView.width, dialogView.height, Bitmap.Config.ARGB_8888)
            dialogView.draw(Canvas(bitmap))
            val file = File("build/reports/mobile-ui/ssh-key-mintlight.png")
            check(file.parentFile!!.isDirectory || file.parentFile!!.mkdirs())
            file.outputStream().use { check(bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)) }
            bitmap.recycle()
        }
    }

    private fun save(name: String, tag: String) {
        val bounds = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot
        compose.runOnIdle {
            val bitmap = Bitmap.createBitmap(bounds.width.toInt(), bounds.height.toInt(), Bitmap.Config.ARGB_8888)
            val canvas = Canvas(bitmap)
            canvas.translate(-bounds.left, -bounds.top)
            checkNotNull(view).draw(canvas)
            val file = File("build/reports/mobile-ui/$name.png")
            check(file.parentFile!!.isDirectory || file.parentFile!!.mkdirs())
            file.outputStream().use { check(bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)) }
            bitmap.recycle()
        }
    }
}
