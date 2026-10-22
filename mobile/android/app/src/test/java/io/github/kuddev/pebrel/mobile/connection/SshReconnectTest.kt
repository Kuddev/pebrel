package io.github.kuddev.pebrel.mobile.connection

import kotlinx.coroutines.*
import kotlinx.coroutines.test.*
import org.junit.Assert.*
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class SshReconnectTest {
    @Test fun backgroundRetriesAreBoundedAndIndependentForEachTab() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val calls = mutableListOf<String>()
        val retry = SshReconnect(this) { calls += it }
        try {
            repeat(10) { retry.schedule("A") }
            retry.schedule("B")
            advanceTimeBy(1001)
            assertEquals(listOf("A", "B"), calls)
            retry.schedule("A")
            advanceTimeBy(1999)
            assertEquals(2, calls.size)
            advanceTimeBy(2)
            assertEquals(listOf("A", "B", "A"), calls)
            retry.schedule("B")
            retry.forget("B")
            advanceUntilIdle()
            assertEquals(3, calls.size)
        } finally { retry.clear(); Dispatchers.resetMain() }
    }

    @Test fun restoredNetworkSupersedesTheOldBackoffWithoutDuplicatingAttempts() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        var calls = 0
        val retry = SshReconnect(this) { calls++ }
        try {
            retry.schedule("A")
            retry.schedule("A", immediate = true)
            advanceTimeBy(2)
            assertEquals(1, calls)
            advanceUntilIdle()
            assertEquals(1, calls)
        } finally { retry.clear(); Dispatchers.resetMain() }
    }

    @Test fun authenticationTrustAndNormalExitNeverLoop() {
        for (failure in SshFailureKind.entries) assertEquals(
            failure.name, failure in setOf(SshFailureKind.NETWORK, SshFailureKind.TIMEOUT,
                SshFailureKind.REFUSED, SshFailureKind.UNKNOWN_HOST), SshReconnect.retryable(failure))
        assertFalse(SshReconnect.retryable(null))
    }

    @Test fun retryCommandsDoNotCreateReplacementMultiplexerSessions() {
        val host = HostProfile("host", "Host", "localhost", 22, "user", sessionMode = SshSessionMode.TMUX)
        assertEquals("exec tmux attach-session -t 'pebrel'", host.reconnectCommand())
        val window = RemoteWindow("@12", "Editor", state = "active", pane = "%27")
        val remote = RemoteSession("tmux", "${'$'}7", "work", "100:200", windows = listOf(window))
        val target = checkNotNull(RemoteSessions.configuredAttachment(host.copy(sessionName = "work"), RemoteInventory("linux", listOf(remote))))
        val command = RemoteSessions.attachCommand(target)
        assertTrue(command.contains("100:200"))
        assertTrue(command.contains("${'$'}7:@12.%27"))
        assertFalse(command.contains("new-session"))
        assertFalse(command.contains("work"))
        assertThrows(IllegalArgumentException::class.java) {
            RemoteSessions.attachCommand(target.copy(window = window.copy(pane = "%27;bad")))
        }
    }
}
