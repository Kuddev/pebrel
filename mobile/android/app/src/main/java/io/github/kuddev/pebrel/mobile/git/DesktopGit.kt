package io.github.kuddev.pebrel.mobile.git

import io.github.kuddev.pebrel.mobile.connection.DesktopConnectionFailure
import io.github.kuddev.pebrel.mobile.connection.DesktopRpcFailure
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import org.json.JSONObject
import java.util.concurrent.atomic.AtomicLong

/** 会话拥有操作回执；退出页面不重发写操作，重连也不把旧请求送给新电脑。 */
class DesktopGit(val target: DesktopGitTarget, private val scope: CoroutineScope,
                 private val request: suspend (String, JSONObject) -> JSONObject,
                 val availability: (Boolean) -> String?, private val committed: (String) -> Unit = {}) {
    private val mutable = MutableStateFlow(GitViewState())
    val state = mutable.asStateFlow()
    private val operation = Mutex()
    private val diffGeneration = AtomicLong()

    private suspend fun rpc(method: String, params: JSONObject = JSONObject(), write: Boolean = false): JSONObject {
        availability(write)?.let { throw GitProblem(it) }
        val reply = try {
            request("git.$method", params.put("window_id", target.window).put("pane_id", target.pane).put("expected_cwd", target.cwd))
        } catch (error: DesktopRpcFailure) {
            throw GitProblem(error.code, error.detail)
        } catch (error: DesktopConnectionFailure) {
            throw GitProblem(if (write) "git_outcome_unknown" else "git_disconnected")
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (_: Exception) {
            throw GitProblem(if (write) "git_outcome_unknown" else "git_disconnected")
        }
        // 已确认的写入回执属于旧目标，不能因随后掉线而伪装成可重试的失败。
        if (!write) availability(false)?.let { throw GitProblem(it) }
        return reply
    }

    private fun run(block: suspend () -> Unit) {
        scope.launch {
            if (!operation.tryLock()) return@launch
            mutable.update { it.copy(busy = true, error = null) }
            try { block() }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (error: Exception) {
                mutable.update { it.copy(error = error as? GitProblem ?: GitProblem("git_failed", error.message.orEmpty().take(4096))) }
            } finally {
                mutable.update { it.copy(busy = false) }
                operation.unlock()
            }
        }
    }

    private suspend fun readStatus(): GitSnapshot = GitSnapshot.parse(rpc("status"))

    fun refresh() = run {
        mutable.update { it.copy(needsRefresh = true) }
        val snapshot = readStatus()
        mutable.update { it.copy(snapshot = snapshot, needsRefresh = false) }
    }

    fun mutate(action: GitAction, path: String = "", all: Boolean = false, message: String = "") = run {
        availability(true)?.let { throw GitProblem(it) }
        var snapshot = mutable.value.snapshot
        if (snapshot == null || mutable.value.needsRefresh) throw GitProblem("git_stale")
        mutable.update { it.copy(completed = emptyList(), needsRefresh = true) }
        val steps = if (action == GitAction.Sync) listOf(GitAction.Fetch, GitAction.Pull, GitAction.Push) else listOf(action)
        for (step in steps) {
            mutable.update { it.copy(needsRefresh = true) }
            val params = JSONObject().put("revision", snapshot!!.revision)
            when (step) {
                GitAction.Stage, GitAction.Unstage -> params.put("path", path).put("all", all)
                GitAction.Commit -> params.put("message", message)
                else -> Unit
            }
            val result = rpc(step.method, params, write = true)
            if (!result.optBoolean("success")) throw GitProblem("git_outcome_unknown")
            if (step == GitAction.Commit) committed(message)
            diffGeneration.incrementAndGet()
            mutable.update { it.copy(completed = it.completed + step, history = emptyList(), hasMore = false, diff = null, diffText = null) }
            try {
                snapshot = readStatus()
                mutable.update { it.copy(snapshot = snapshot, needsRefresh = false) }
            } catch (error: Exception) {
                if (error is CancellationException) throw error
                throw GitProblem("git_refresh_failed", (error as? GitProblem)?.detail.orEmpty())
            }
        }
    }

    fun history(more: Boolean = false) = run {
        val previous = if (more) mutable.value.history else emptyList()
        val result = rpc("history", JSONObject().put("offset", previous.size))
        val commits = result.getJSONArray("commits")
        val next = List(commits.length()) { index -> commits.getJSONObject(index).let { item ->
            GitCommit(item.getString("hash"), item.getString("short_hash"), item.getString("subject"),
                item.getString("author"), item.getLong("timestamp"), item.optString("refs"))
        } }
        mutable.update { it.copy(history = (previous + next).distinctBy(GitCommit::hash), hasMore = result.optBoolean("has_more")) }
    }

    fun openDiff(selection: GitDiffSelection) {
        val generation = diffGeneration.get()
        run {
            if (generation != diffGeneration.get()) return@run
            mutable.update { it.copy(diff = selection, diffText = null) }
            val result = rpc("diff", JSONObject().put("path", selection.path).put("staged", selection.staged).put("commit", selection.commit))
            if (generation == diffGeneration.get()) mutable.update { it.copy(diffText = result.getString("text")) }
        }
    }

    fun closeDiff() {
        diffGeneration.incrementAndGet()
        mutable.update { it.copy(diff = null, diffText = null, error = null) }
    }
}
