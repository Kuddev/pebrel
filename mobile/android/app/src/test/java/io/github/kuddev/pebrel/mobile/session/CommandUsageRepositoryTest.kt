package io.github.kuddev.pebrel.mobile.session

import android.app.Application
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.ConscryptMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], application = Application::class)
@ConscryptMode(ConscryptMode.Mode.OFF)
class CommandUsageRepositoryTest {
    @Test fun acceptedSubmissionsUpdateVisibleOrderAndClosingTheOwnerDropsItsCounts() {
        val repository = SessionRepository(ApplicationProvider.getApplicationContext())
        try {
            repository.setDraft("ssh:a", "git status")
            repeat(2) { repository.acknowledgeDraft("ssh:a", "git status") }
            repository.acknowledgeDraft("ssh:a", "git pull")
            repository.acknowledgeDraft("ssh:b", "git pull")
            assertFalse(repository.drafts.value.containsKey("ssh:a"))
            assertEquals(listOf("git status", "git pull"), repository.commandHistory.value["ssh:a"])
            assertEquals(listOf("git pull"), repository.commandHistory.value["ssh:b"])
            repository.closeTerminal("ssh:a")
            repository.acknowledgeDraft("ssh:a", "git status")
            repository.acknowledgeDraft("ssh:a", "git pull")
            assertEquals(listOf("git pull", "git status"), repository.commandHistory.value["ssh:a"])
            repository.closeDesktop("ssh")
            assertTrue(repository.commandHistory.value.isEmpty())
        } finally { repository.closeAll() }
    }
}
