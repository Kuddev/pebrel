package io.github.kuddev.pebrel.mobile.session

import android.app.Application
import android.os.Looper
import androidx.test.core.app.ApplicationProvider
import io.github.kuddev.pebrel.mobile.connection.*
import io.github.kuddev.pebrel.terminal.*
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.*
import java.time.Duration
import kotlinx.coroutines.*

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], application = Application::class, shadows = [SshRecoveryTest.Terminal::class],
    instrumentedPackages = ["io.github.kuddev.pebrel.terminal"])
@ConscryptMode(ConscryptMode.Mode.OFF)
class SshRecoveryTest {
    @Implements(value = TerminalSession::class, isInAndroidSdk = false)
    class Terminal {
        @RealObject lateinit var real: TerminalSession
        lateinit var callbacks: TerminalCallbacks
        var closed = false
        var inputRestorations = 0
        @Implementation fun __constructor__(transport: SessionTransport, callbacks: TerminalCallbacks, scrollbackLines: Int) {
            this.callbacks = callbacks
        }
        @Implementation fun start() { started += this }
        @Implementation fun finishIfRunning() { closed = true }
        @Implementation fun colors(value: IntArray) = Unit
        @Implementation fun setVisible(value: Boolean) = Unit
        @Implementation fun updateSize(columns: Int, rows: Int, width: Int, height: Int) = Unit
        @Implementation fun prepareForInput() { inputRestorations++ }
        companion object { val started = mutableListOf<Terminal>() }
    }

    private fun await(condition: () -> Boolean) {
        repeat(500) {
            shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(10))
            if (condition()) return
            Thread.sleep(10)
        }
        fail("session transition did not complete")
    }

    @Test fun retryReplacesOnlyTheTransportAndStaleCallbacksCannotCloseItsReplacement() {
        Terminal.started.clear()
        val repository = SessionRepository(ApplicationProvider.getApplicationContext())
        val host = HostProfile("fixture", "Fixture", "localhost", 22, "user")
        val target = RemoteAttachment(RemoteSession("tmux", "${'$'}3", "work", "10:20"), RemoteWindow("@4", "A", pane = "%5"))
        try {
            val a = repository.ssh(host, charArrayOf(), target)
            val old = Terminal.started.last()
            old.callbacks.onTransportReady(old.real)
            val b = repository.ssh(host, charArrayOf())
            val other = Terminal.started.last()
            other.callbacks.onTransportReady(other.real)
            repository.setDraft(a, "unfinished")
            old.callbacks.onSessionFinished(old.real)
            repository.retrySsh(a)
            await { Terminal.started.size == 3 }
            val replacement = Terminal.started.last()
            assertTrue(old.closed)
            assertFalse(other.closed)
            assertEquals(listOf(a, b), repository.sessions.value.map { it.id })
            assertEquals(target, repository.sessions.value.first().attachment)
            assertEquals("unfinished", repository.drafts.value[a])
            old.callbacks.onSessionFinished(old.real)
            old.callbacks.onTransportReady(old.real)
            assertEquals("connecting", repository.sessions.value.first().status)
            replacement.callbacks.onTransportReady(replacement.real)
            assertEquals("ready", repository.sessions.value.first().status)
            assertSame(other.real, repository.sessions.value.last().terminal)
        } finally { repository.closeAll() }
    }

    @Test fun newTmuxTabKeepsThePreviousPtyAliveAndUsesADifferentRemoteSession() {
        Terminal.started.clear()
        val repository = SessionRepository(ApplicationProvider.getApplicationContext())
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
        try {
            val firstId = repository.ssh(HostProfile("fixture", "Fixture", "localhost", 22, "user",
                sessionMode = SshSessionMode.TMUX, sessionName = "work"), charArrayOf())
            val first = Terminal.started.last()
            first.callbacks.onTransportReady(first.real)
            var newId: String? = null
            scope.launch { newId = repository.newSshTab(firstId) }
            await { newId != null }
            assertFalse(first.closed)
            val tabs = repository.sessions.value
            assertEquals(2, tabs.size)
            assertEquals("work", tabs.first().host?.sessionName)
            assertNotEquals("work", tabs.last().host?.sessionName)
            assertNotEquals(firstId, newId)
            repository.closeTerminal(checkNotNull(newId))
            assertFalse(first.closed)
            assertSame(first.real, repository.sessions.value.single().terminal)
        } finally { scope.cancel(); repository.closeAll() }
    }

    @Test fun openingTheKeyboardRestoresLiveInputBeforeAnyTextIsCommitted() {
        Terminal.started.clear()
        val repository = SessionRepository(ApplicationProvider.getApplicationContext())
        val activity = org.robolectric.Robolectric.buildActivity(android.app.Activity::class.java).setup()
        try {
            repository.ssh(HostProfile("fixture", "Fixture", "localhost", 22, "user"), charArrayOf())
            val terminal = Terminal.started.last()
            val view = GhosttyView(activity.get()).apply { session = terminal.real; directInput = true }
            activity.get().setContentView(view)
            view.layout(0, 0, 600, 800)
            view.showKeyboard()
            assertTrue(terminal.inputRestorations > 0)
            assertTrue(view.hasFocus())
        } finally { activity.pause().stop().destroy(); repository.closeAll() }
    }

    @Test fun returningToForegroundProbesAStaleSocketWithoutChangingTheSelectedTab() {
        Terminal.started.clear()
        val repository = SessionRepository(ApplicationProvider.getApplicationContext())
        try {
            val id = repository.ssh(HostProfile("fixture", "Fixture", "localhost", 22, "user"), charArrayOf())
            val old = Terminal.started.last()
            old.callbacks.onTransportReady(old.real)
            // The shadow terminal has no native SSH handle: the resume query reports CLOSED.
            repository.foregroundChanged(true)
            await { Terminal.started.size == 2 }
            assertEquals(id, repository.sessions.value.single().id)
            assertTrue(old.closed)
        } finally { repository.foregroundChanged(false); repository.closeAll() }
    }

    @Test fun routeRecoveryRunsInTheBackgroundAndClosingATabCancelsItsRetry() {
        Terminal.started.clear()
        val repository = SessionRepository(ApplicationProvider.getApplicationContext())
        try {
            val id = repository.ssh(HostProfile("fixture", "Fixture", "localhost", 22, "user"), charArrayOf())
            val first = Terminal.started.last()
            first.callbacks.onTransportReady(first.real)
            repository.foregroundChanged(false)
            repository.networkChanged()
            await { Terminal.started.size == 2 }
            assertEquals(id, repository.sessions.value.single().id)
            val second = Terminal.started.last()
            second.callbacks.onTransportReady(second.real)
            repository.networkChanged()
            repository.closeTerminal(id)
            shadowOf(Looper.getMainLooper()).idleFor(Duration.ofSeconds(60))
            assertEquals(2, Terminal.started.size)
            assertTrue(repository.sessions.value.isEmpty())
        } finally { repository.closeAll() }
    }
}
