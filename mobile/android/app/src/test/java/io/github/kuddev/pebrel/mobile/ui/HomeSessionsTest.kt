package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import io.github.kuddev.pebrel.mobile.connection.*
import io.github.kuddev.pebrel.mobile.session.DesktopWorkspace
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class HomeSessionsTest {
    @get:Rule val compose = createComposeRule()
    private val pane = DesktopPane(1, 2, "PC shell", "/project", "pwsh", "idle", 1)
    private val desktop = DesktopWorkspace("pc", HostProfile("host", "My computer", "192.0.2.1", 22, "root"),
        panes = listOf(pane), status = "ready", transport = "Relay", hasConnected = true)

    @Test fun connectedPcAppearsInSessionGalleryAndOpensExactPane() {
        var opened = ""
        compose.setContent { MaterialTheme { HomeScreen(
            emptyList(), emptyList(), listOf(desktop), emptyList(), {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {},
            onPane = { computer, target -> opened = "$computer:${target.window}:${target.id}" }) } }
        compose.onNodeWithText("PC shell").assertIsDisplayed().performClick()
        compose.runOnIdle { assertEquals("pc:1:2", opened) }
    }

    @Test fun disconnectedPcRemainsButFailedFirstAttemptDoesNotBecomeASession() {
        val cards = sessionCards(emptyList(), listOf(desktop.copy(status = "disconnected"),
            desktop.copy(id = "failed", status = "failed", hasConnected = false)))
        assertEquals(listOf("pc:pc:1:2"), cards.map { it.key })
    }
}
