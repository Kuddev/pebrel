package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.unit.dp
import io.github.kuddev.pebrel.mobile.git.*
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], qualifiers = "w360dp-h640dp-mdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class GitScreenTest {
    @get:Rule val compose = createComposeRule()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private val root = "D:/a-very-long-parent/another-folder/work"
    private val target = DesktopGitTarget("pc", 1, 2, root, 3, 4)
    private val calls = mutableListOf<Pair<String, JSONObject>>()
    private var draft by mutableStateOf("")

    @After fun close() { scope.cancel() }

    private fun show(readonly: Boolean = false) {
        val controller = DesktopGit(target, scope, { method, params ->
            calls += method to JSONObject(params.toString())
            when (method) {
                "git.status" -> JSONObject("""{"root":"$root","revision":"${"a".repeat(64)}",
                    "status":{"branch":"main","upstream":"origin/main","ahead":0,"behind":0,
                    "entries":[
                    {"path":"README.md","original":null,"index":".","worktree":"M","conflict":false},
                    {"path":"src/Main.kt","original":null,"index":".","worktree":"M","conflict":false},
                    {"path":"new file.txt","original":null,"index":"A","worktree":".","conflict":false}]}}""")
                "git.history" -> JSONObject("""{"commits":[],"has_more":false}""")
                "git.diff" -> JSONObject().put("text", "+changed")
                else -> JSONObject().put("success", true)
            }
        }, { write -> if (write && readonly) "input_not_authorized" else null })
        compose.setContent {
            val state by controller.state.collectAsState()
            LaunchedEffect(controller) { controller.refresh() }
            MaterialTheme {
                Box(Modifier.requiredSize(360.dp, 600.dp).testTag("git-page")) {
                    GitScreenContent(target, "Computer", state, null, !readonly, draft,
                        { draft = it }, controller, {})
                }
            }
        }
        compose.waitForIdle()
    }

    @Test fun compactPageShowsFilesBeforeTheDockedCommitBarAndKeepsFullPathInDetails() {
        show()
        compose.onNodeWithText("work").assertIsDisplayed()
        compose.onNodeWithText(root).assertDoesNotExist()
        val file = compose.onNodeWithContentDescription("README.md").assertIsDisplayed().getUnclippedBoundsInRoot()
        val input = compose.onNodeWithContentDescription("Commit message").getUnclippedBoundsInRoot()
        assertTrue("files must not be hidden below the commit form", file.bottom < input.top)
        assertTrue("commit bar is docked, not at the top of the list", input.top > 490.dp)
        compose.onNodeWithContentDescription("Refresh").assertHeightIsAtLeast(48.dp).assertWidthIsAtLeast(48.dp)
        compose.onNodeWithContentDescription("Stage README.md").assertHeightIsAtLeast(48.dp).assertWidthIsAtLeast(48.dp)
        compose.onNodeWithContentDescription("Repository details").performTouchInput { click() }
        compose.onNodeWithText(root).assertIsDisplayed()
    }

    @Test fun draftAndCollapsedFolderSurviveHistoryAndOnlyCommitButtonSubmits() {
        show()
        compose.onNodeWithContentDescription("Commit staged changes (1)").assertIsNotEnabled()
        compose.onNodeWithContentDescription("Commit message").performTextInput("Review from phone")
        assertTrue(calls.none { it.first == "git.commit" })
        compose.onNodeWithContentDescription("src").performTouchInput { click() }
        compose.onNodeWithContentDescription("src/Main.kt").assertDoesNotExist()
        compose.onNodeWithText("History").performTouchInput { click() }
        compose.onNodeWithText("Changes", useUnmergedTree = false).performTouchInput { click() }
        compose.onNodeWithContentDescription("src/Main.kt").assertDoesNotExist()
        compose.onNodeWithText("Review from phone").assertIsDisplayed()
        compose.onNodeWithContentDescription("Commit staged changes (1)").assertIsEnabled().performTouchInput { click() }
        compose.runOnIdle {
            val request = calls.filter { it.first == "git.commit" }.single().second
            assertEquals("Review from phone", request.getString("message"))
            assertEquals(root, request.getString("expected_cwd"))
        }
    }

    @Test fun readOnlyConnectionKeepsDiffReadableButAllMutationEntriesDisabled() {
        show(readonly = true)
        compose.onNodeWithText("Stage all").assertIsNotEnabled()
        compose.onNodeWithText("Unstage all").assertIsNotEnabled()
        compose.onNodeWithContentDescription("Stage README.md").assertIsNotEnabled()
        compose.onNodeWithContentDescription("Commit message").performTextInput("Draft only")
        compose.onNodeWithContentDescription("Commit staged changes (1)").assertIsNotEnabled()
        compose.onNodeWithContentDescription("README.md").performTouchInput { click() }
        compose.onNodeWithText("+changed").assertIsDisplayed()
        compose.runOnIdle { assertTrue(calls.none { it.first in setOf("git.commit", "git.stage", "git.unstage") }) }
    }
}
