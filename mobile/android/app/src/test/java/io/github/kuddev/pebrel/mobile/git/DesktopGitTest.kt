package io.github.kuddev.pebrel.mobile.git

import io.github.kuddev.pebrel.mobile.connection.*
import io.github.kuddev.pebrel.mobile.session.DesktopWorkspace
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.*
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@OptIn(ExperimentalCoroutinesApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class DesktopGitTest {
    private val target = DesktopGitTarget("pc", 1, 2, "/repo", 3, 4)
    private fun status() = JSONObject("""{"root":"/repo","revision":"${"a".repeat(64)}",
        "status":{"branch":"main","head":"abc","upstream":"origin/main","ahead":0,"behind":0,
        "entries":[{"path":"src/中文 [1].txt","original":null,"index":"M","worktree":"M","conflict":false}]}}""")

    @Test fun readOnlyAndChangedIdentityRejectWritesBeforeTransport() = runTest {
        val calls = mutableListOf<String>()
        val controller = DesktopGit(target, this, { method, _ -> calls += method; status() }, { write -> if (write) "input_not_authorized" else null })
        controller.refresh(); runCurrent()
        controller.mutate(GitAction.Stage, all = true); runCurrent()
        assertEquals(listOf("git.status"), calls)
        assertEquals("input_not_authorized", controller.state.value.error?.code)
        val host = HostProfile("pc", "Computer", "localhost", user = "test")
        val pane = DesktopPane(1, 2, "Shell", "/repo", "", "idle", 0)
        val current = DesktopWorkspace("pc", host, listOf(pane), status = "ready", allowInput = true, connectionGeneration = 3, runtimeProcess = 4)
        assertNull(target.availability(current, true))
        assertEquals("git_target_changed", target.availability(current.copy(connectionGeneration = 5), true))
        assertEquals("git_target_changed", target.availability(current.copy(panes = listOf(pane.copy(cwd = "/other"))), false))
        assertEquals("git_target_changed", target.availability(current.copy(panes = listOf(pane.copy(sshDestination = "remote"))), false))
        assertEquals("git_disconnected", target.availability(current.copy(status = "disconnected"), false))
    }

    @Test fun successfulCommitAndFailedRefreshKeepReceiptAndNeverRepeatCommit() = runTest {
        var reads = 0
        var writes = 0
        val committed = mutableListOf<String>()
        val controller = DesktopGit(target, this, { method, _ ->
            if (method == "git.status") {
                if (++reads > 1) throw DesktopRpcFailure("git_failed", "refresh failed")
                status()
            } else { writes++; JSONObject("""{"success":true}""") }
        }, { null }, committed::add)
        controller.refresh(); runCurrent()
        controller.mutate(GitAction.Commit, message = "from mobile"); runCurrent()
        assertEquals(listOf("from mobile"), committed)
        assertEquals(listOf(GitAction.Commit), controller.state.value.completed)
        assertEquals("git_refresh_failed", controller.state.value.error?.code)
        assertTrue(controller.state.value.needsRefresh)
        controller.mutate(GitAction.Commit, message = "from mobile"); runCurrent()
        assertEquals(1, writes)
    }

    @Test fun unknownDeliveryAndPartialSyncRequireExplicitRefresh() = runTest {
        val writes = mutableListOf<String>()
        val controller = DesktopGit(target, this, { method, _ ->
            when (method) {
                "git.status" -> status()
                "git.fetch" -> { writes += method; JSONObject("""{"success":true}""") }
                else -> { writes += method; throw DesktopConnectionFailure(DesktopFailureKind.NETWORK) }
            }
        }, { null })
        controller.refresh(); runCurrent()
        controller.mutate(GitAction.Sync); runCurrent()
        assertEquals(listOf("git.fetch", "git.pull"), writes)
        assertEquals(listOf(GitAction.Fetch), controller.state.value.completed)
        assertEquals("git_outcome_unknown", controller.state.value.error?.code)
        assertTrue(controller.state.value.needsRefresh)
        controller.refresh(); runCurrent()
        assertEquals(2, writes.size)
    }

    @Test fun closingDiffDropsLateResponseAndTreeKeepsExactPaths() = runTest {
        val reply = CompletableDeferred<JSONObject>()
        val controller = DesktopGit(target, this, { _, _ -> reply.await() }, { null })
        controller.openDiff(GitDiffSelection("src/中文 [1].txt")); runCurrent()
        controller.closeDiff()
        reply.complete(JSONObject().put("text", "+late response")); runCurrent()
        assertNull(controller.state.value.diff)
        assertNull(controller.state.value.diffText)
        val entries = GitSnapshot.parse(status()).entries
        assertEquals(listOf("src", "src/中文 [1].txt"), gitTree(entries, emptySet()).map { it.path })
        assertEquals(listOf("src"), gitTree(entries, setOf("src")).map { it.path })
        assertTrue(entries.single().staged && entries.single().unstaged)
    }
}
