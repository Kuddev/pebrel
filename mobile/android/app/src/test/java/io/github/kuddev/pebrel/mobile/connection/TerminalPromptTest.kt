package io.github.kuddev.pebrel.mobile.connection

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class TerminalPromptTest {
    private val approval = """
        ╭──────────────────────────────────────────────╮
        │ Do you want to make this edit to main.rs?    │
        │ ❯ 1. Yes                                     │
        │   2. Yes, allow all edits during this session │
        │   3. No, and tell Claude what to do (esc)     │
        ╰──────────────────────────────────────────────╯
    """.trimIndent()

    @Test fun aBoxedApprovalMenuBecomesOptionsWithTheMarkedOneSelected() {
        val prompt = TerminalPrompt.parse(approval)!!
        assertEquals("Do you want to make this edit to main.rs?", prompt.text)
        assertEquals(listOf("Yes", "Yes, allow all edits during this session", "No, and tell Claude what to do (esc)"),
            prompt.options)
        assertEquals(0, prompt.selected)
    }

    @Test fun movingTheMarkerKeepsTheSameQuestionButAnswersChangeTheIdentity() {
        val moved = approval.replace("│ ❯ 1. Yes", "│   1. Yes").replace("│   2. Yes,", "│ ❯ 2. Yes,")
        val first = TerminalPrompt.parse(approval)!!
        val second = TerminalPrompt.parse(moved)!!
        assertEquals(1, second.selected)
        assertEquals(first.id, second.id)
        assertNotEquals(first.id, TerminalPrompt.parse(approval.replace("main.rs", "lib.rs"))!!.id)
    }

    @Test fun aQuestionWrappedByTheTerminalIsJoinedBack() {
        val screen = " Do you want to make this edit to mai\nn.rs?\n ❯ 1. Yes\n   2. No\n" + "x".repeat(37)
        assertEquals("Do you want to make this edit to main.rs?", TerminalPrompt.parse(screen)!!.text)
    }

    @Test fun numberedProseAndListsWithoutAMarkerAreNotMenus() {
        assertNull(TerminalPrompt.parse("Steps:\n1. Build\n2. Test\n3. Ship"))
        assertNull(TerminalPrompt.parse("❯ 2. Second\n  3. Third"))
        assertNull(TerminalPrompt.parse("$ ls\nREADME.md"))
    }

    @Test fun onlyARecentInterruptHintMeansTheAgentIsRunning() {
        assertTrue(TerminalPrompt.isRunning("✻ Thinking… (12s · esc to interrupt)\n> "))
        assertFalse(TerminalPrompt.isRunning("esc to interrupt\n" + "line\n".repeat(20)))
    }

    @Test fun everyKnownAgentHasADisplayName() {
        assertEquals(listOf("Claude Code", "Codex", "Pi", "Antigravity"),
            listOf("claude", "codex", "pi", "antigravity").map(::sshAgentName))
    }
}
