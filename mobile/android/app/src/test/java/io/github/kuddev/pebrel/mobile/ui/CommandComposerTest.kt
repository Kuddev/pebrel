package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.*
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.test.core.app.ApplicationProvider
import io.github.kuddev.pebrel.mobile.PebrelApplication
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.session.SessionRepository
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class CommandComposerTest {
    @get:Rule val compose = createComposeRule()

    @Test fun oneSlotSwitchesModesWithoutLosingDraftOrKeepingAnEmptyToolbar() {
        val context = ApplicationProvider.getApplicationContext<PebrelApplication>()
        val repository = SessionRepository(context)
        compose.setContent {
            var direct by remember { mutableStateOf(true) }
            MaterialTheme {
                CommandComposer("test-session", repository, true, direct, { direct = it }, {}, onKeyboard = {}) { true }
            }
        }
        compose.onNodeWithTag("composer-direct").assertExists()
        compose.onNodeWithTag("composer-editor").assertDoesNotExist()
        compose.onAllNodes(hasSetTextAction()).assertCountEquals(0)
        compose.onNodeWithContentDescription(context.getString(R.string.composer_mode_edit)).performClick()
        compose.onNodeWithTag("composer-direct").assertDoesNotExist()
        compose.onNodeWithTag("composer-editor").assertExists()
        compose.onAllNodes(hasSetTextAction()).assertCountEquals(1)
        compose.onNode(hasSetTextAction()).performTextInput("echo preserved")
        compose.onNodeWithContentDescription(context.getString(R.string.composer_mode_direct)).performClick()
        compose.onNodeWithTag("composer-editor").assertDoesNotExist()
        compose.onNodeWithTag("composer-direct").assertExists()
        compose.onNodeWithContentDescription(context.getString(R.string.composer_mode_edit)).performClick()
        compose.onNode(hasSetTextAction()).assertTextEquals("echo preserved")
        compose.onNodeWithTag("composer-direct").assertDoesNotExist()
    }

    @Test fun desktopComposerHasOneEditorAndNoPermanentShortcutSurface() {
        val context = ApplicationProvider.getApplicationContext<PebrelApplication>()
        val repository = SessionRepository(context)
        compose.setContent {
            MaterialTheme { CommandComposer("desktop", repository, true, false, null, null) { true } }
        }
        compose.onNodeWithTag("composer-editor").assertExists()
        compose.onNodeWithTag("composer-direct").assertDoesNotExist()
        compose.onAllNodes(hasSetTextAction()).assertCountEquals(1)
    }
}
