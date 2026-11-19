package io.github.kuddev.pebrel.mobile.connection

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class SshForegroundTest {
    @Test fun anAgentInATmuxPaneIsNamedWithItsShortenedDirectory() {
        val seen = SshForeground.parse(JSONObject(
            """{"terminal":"pts/3","multiplexer":"tmux","pane":"%0","agent":"claude","program":"claude","cwd":"/home/u/src/app","home":"/home/u","version":1}"""))
        assertEquals(SshForeground("claude", "~/src/app", "claude", "tmux"), seen)
        assertEquals("Claude Code · ~/src/app", seen.label)
    }

    @Test fun anIdlePromptShowsOnlyItsDirectoryAndNullsStayAbsent() {
        val seen = SshForeground.parse(JSONObject(
            """{"terminal":"pts/3","multiplexer":null,"pane":null,"agent":null,"program":"","cwd":"/home/u","home":"/home/u/","version":1}"""))
        assertNull(seen.agent)
        assertNull(seen.multiplexer)
        assertEquals("~", seen.label)
        // A sibling of the home directory is not inside it.
        assertEquals("/home/user2 · vim", SshForeground.parse(JSONObject(
            """{"agent":null,"program":"vim","cwd":"/home/user2","home":"/home/u"}""")).let { "${it.cwd} · ${it.program}" })
        assertEquals("htop", SshForeground.parse(JSONObject("""{"agent":null,"program":"htop","cwd":null,"home":"/root"}""")).label)
    }
}
