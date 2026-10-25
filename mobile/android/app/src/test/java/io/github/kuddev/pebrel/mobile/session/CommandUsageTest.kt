package io.github.kuddev.pebrel.mobile.session

import org.junit.Assert.*
import org.junit.Test

class CommandUsageTest {
    @Test fun acceptedCommandsRankByFrequencyThenRecency() {
        val history = CommandUsage()
        history.record("git status")
        history.record("git status")
        assertEquals(listOf("git status", "git pull"), history.record("git pull"))
        assertEquals(listOf("git pull", "git status"), history.record("git pull"))
        assertEquals(listOf("git status", "git pull"), history.record("git status"))
        assertEquals(listOf("git status", "git pull"), history.record(" "))
        assertEquals(listOf("git pull"), CommandUsage().record("git pull"))
    }

    @Test fun limitsEvictLeastRecentlyUsedCommandsAndTheirCountsTogether() {
        val history = CommandUsage()
        repeat(10) { history.record("stale") }
        repeat(40) { history.record("command $it") }
        val ranked = history.record("stale")
        assertEquals(40, ranked.size)
        assertEquals(listOf("stale", "command 39"), ranked.take(2))
        assertFalse(ranked.contains("command 0"))
        assertEquals(ranked, history.record("x".repeat(32_769)))
        assertEquals(listOf("x".repeat(32_768)), history.record("x".repeat(32_768)))
    }
}
