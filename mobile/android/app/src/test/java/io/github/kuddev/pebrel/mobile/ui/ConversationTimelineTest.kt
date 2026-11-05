package io.github.kuddev.pebrel.mobile.ui

import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import androidx.test.core.app.ApplicationProvider
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.ConversationMessage
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], qualifiers = "w360dp-h800dp")
class ConversationTimelineTest {
    @get:Rule val compose = createComposeRule()
    private val style = ChatMarkdownStyle(Color.Blue, Color.LightGray, FontFamily.Monospace, "Image")

    private fun message(id: String, role: String, text: String, name: String? = null, detail: String? = null,
                        complete: Boolean = true, error: Boolean = false) =
        ConversationMessage(id, role, text, name, detail, complete, false, error)

    @Test fun claudeAndCodexToolNamesShareKinds() {
        assertEquals(ToolKind.Shell, toolKind("Bash"))
        assertEquals(ToolKind.Shell, toolKind("exec_command"))
        assertEquals(ToolKind.Edit, toolKind("apply_patch"))
        assertEquals(ToolKind.Edit, toolKind("MultiEdit"))
        assertEquals(ToolKind.Read, toolKind("mcp__filesystem__read_file"))
        assertEquals(ToolKind.Question, toolKind("AskUserQuestion"))
        assertEquals(ToolKind.Other, toolKind(null))
        assertEquals("github · search_code", toolDisplayName("mcp__github__search_code"))
        assertEquals("Bash", toolDisplayName("Bash"))
    }

    @Test fun summariesComeFromTheCallsOwnArguments() {
        assertEquals("ls -la", toolSummary(ToolKind.Shell, """{"command": "ls -la\necho done"}"""))
        assertEquals("src/A.kt, B.kt", toolSummary(ToolKind.Edit, "*** Begin Patch\n*** Update File: src/A.kt\n*** Add File: B.kt\n"))
        assertEquals("TODO · src", toolSummary(ToolKind.Search, """{"pattern": "TODO", "path": "src"}"""))
        assertEquals("Fix the parser", toolSummary(ToolKind.Todo,
            """{"todos": [{"content": "Write tests", "status": "completed"}, {"content": "Fix the parser", "status": "in_progress"}]}"""))
        val long = toolSummary(ToolKind.Shell, """{"command": "${"x".repeat(400)}"}""")
        assertEquals(160, long.length)
        assertTrue(long.endsWith("…"))
        // Structured values without a known field are not guessed from.
        assertEquals("", toolSummary(ToolKind.Other, "[1, 2]"))
    }

    @Test fun structuredQuestionsKeepTheirOptions() {
        val questions = askedQuestions("""{"questions": [{"header": "Scope", "question": "Which files?", "multiSelect": true,
            "options": [{"label": "All", "description": "Every module"}, "Only UI", {"description": "no label"}]},
            {"question": "  "}]}""")
        assertEquals(listOf(AskedQuestion("Scope", "Which files?",
            listOf(AskedOption("All", "Every module"), AskedOption("Only UI", "")), true)), questions)
        assertNull(askedQuestions("not json"))
        assertNull(askedQuestions("""{"questions": []}"""))
        val open = toolItem(message("q", "tool", """{"questions": [{"question": "Ship it?", "options": ["Yes", "No"]}]}""",
            "AskUserQuestion", complete = false), "waiting_input")
        assertEquals(ChatItem.Question("q", listOf(AskedQuestion("", "Ship it?",
            listOf(AskedOption("Yes", ""), AskedOption("No", "")), false)), answered = false), open)
        // Arguments in another shape stay an ordinary tool row instead of disappearing.
        assertTrue(toolItem(message("q2", "tool", "plain", "AskUserQuestion"), "idle") is ChatItem.Tool)
    }

    @Test fun turnsAndToolStatesFollowTheTranscript() {
        val items = ConversationTimelineBuilder(style).build(listOf(
            message("u1", "user", "Run the tests"),
            message("a1", "assistant", "Running them now."),
            message("t1", "tool", """{"command": "gradle test"}""", "Bash", "BUILD FAILED", error = true),
            message("t2", "tool", """{"command": "gradle test"}""", "Bash", "ok"),
            message("a2", "assistant", "Fixed."),
            message("t3", "tool", """{"command": "sleep 1"}""", "Bash", complete = false),
        ), "running")
        assertTrue((items[1] as ChatItem.Assistant).startsTurn)
        assertFalse((items[4] as ChatItem.Assistant).startsTurn)
        assertEquals(listOf(ToolState.Failed, ToolState.Done, ToolState.Running), items.filterIsInstance<ChatItem.Tool>().map { it.state })
        assertFalse("a running call shows its own progress", showsWorking(items, "running"))
        assertTrue(showsWorking(items.dropLast(1), "running"))
        assertFalse(showsWorking(items.dropLast(1), "idle"))
        val unfinished = message("t4", "tool", "{}", "Bash", complete = false)
        assertEquals(ToolState.Waiting, (toolItem(unfinished, "waiting_input") as ChatItem.Tool).state)
        assertEquals(ToolState.Pending, (toolItem(unfinished, "idle") as ChatItem.Tool).state)
    }

    @Test fun unchangedMessagesKeepTheirParsedDocument() {
        val builder = ConversationTimelineBuilder(style)
        val first = builder.build(listOf(message("u1", "user", "Hello"), message("a1", "assistant", "Partial")), "running")
        val second = builder.build(listOf(message("u1", "user", "Hello"), message("a1", "assistant", "Partial answer")), "running")
        assertSame((first[0] as ChatItem.User).document, (second[0] as ChatItem.User).document)
        assertNotSame((first[1] as ChatItem.Assistant).document, (second[1] as ChatItem.Assistant).document)
    }

    @Test fun markdownKeepsStructureAndTexSource() {
        val plain = parseChatMarkdown("# Plan\n\n- [x] parse\n- [ ] render\n\n| a | b |\n|:--|--:|\n| 1 | 2 |\n\n```kotlin\nval x = 1\n```", style)
        assertFalse(plain.rich)
        val heading = plain.blocks[0] as ChatBlock.Heading
        assertEquals(1 to "Plan", heading.level to heading.text.text)
        assertEquals(listOf(true, false), (plain.blocks[1] as ChatBlock.Items).items.map { it.checked })
        val table = plain.blocks[2] as ChatBlock.Table
        assertEquals(listOf(TextAlign.Start, TextAlign.End), table.alignments)
        assertEquals(listOf("1", "2"), table.rows.single().map { it.text })
        assertEquals(ChatBlock.Code("kotlin", "val x = 1"), plain.blocks[3])

        val math = parseChatMarkdown("Energy \$e_k = m_1\$ here.\n\n\$\$\n\\frac{a}{b}\n\$\$", style)
        assertTrue(math.rich)
        // Underscores inside TeX are not read as Markdown emphasis.
        assertEquals("Energy e_k = m_1 here.", (math.blocks[0] as ChatBlock.Paragraph).text.text)
        assertEquals(ChatBlock.Math("\\frac{a}{b}"), math.blocks[1])
    }

    @Test fun toolCallsStartCollapsedAndCopyConfirmsThenRecovers() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val items = ConversationTimelineBuilder(style).build(listOf(
            message("u1", "user", "Check the build"),
            message("a1", "assistant", "Run this:\n\n```sh\ngradle check\n```"),
            message("t1", "tool", """{"command": "gradle test"}""", "Bash", "BUILD SUCCESSFUL"),
        ), "idle")
        compose.setContent {
            MaterialTheme {
                val clipboard = rememberReaderClipboard("test") {}
                ChatTimeline(items, false, "Codex", rememberLazyListState(), PaddingValues(), hasOlder = false,
                    olderEnabled = false, onOlder = {}, actions = ChatActions(clipboard, {}, {}), modifier = Modifier.fillMaxSize())
            }
        }
        compose.onNodeWithText("Check the build").assertIsDisplayed()
        compose.onNodeWithText("Codex").assertIsDisplayed()
        compose.onAllNodesWithText("BUILD SUCCESSFUL").assertCountEquals(0)
        compose.onNodeWithText("Bash").performClick()
        compose.onNodeWithText("BUILD SUCCESSFUL").assertIsDisplayed()
        compose.onNodeWithText(context.getString(R.string.chat_tool_output)).assertIsDisplayed()

        val copied = context.getString(R.string.reader_copied)
        compose.mainClock.autoAdvance = false
        compose.onAllNodesWithText(context.getString(R.string.reader_copy))[0].performClick()
        compose.mainClock.advanceTimeByFrame()
        compose.onNodeWithText(copied).assertIsDisplayed()
        assertEquals("gradle check", context.getSystemService(ClipboardManager::class.java).primaryClip!!.getItemAt(0).text)
        compose.mainClock.advanceTimeBy(1_000)
        compose.onNodeWithText(copied).assertIsDisplayed()
        compose.mainClock.advanceTimeBy(700)
        compose.onAllNodesWithText(copied).assertCountEquals(0)
    }
}
