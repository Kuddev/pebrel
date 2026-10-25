package io.github.kuddev.pebrel.mobile.git

import io.github.kuddev.pebrel.mobile.session.DesktopWorkspace
import org.json.JSONObject

data class DesktopGitTarget(val desktop: String, val window: Long, val pane: Long, val cwd: String,
                            val generation: Long, val process: Long?) {
    val draftKey: String get() = "$desktop:git:$process:$window:$pane:$cwd"

    fun availability(current: DesktopWorkspace?, write: Boolean): String? = when {
        current?.status != "ready" -> "git_disconnected"
        current.connectionGeneration != generation || current.runtimeProcess != process -> "git_target_changed"
        current.panes.none { it.window == window && it.id == pane && it.cwd == cwd && it.sshDestination == null } -> "git_target_changed"
        write && !current.allowInput -> "input_not_authorized"
        else -> null
    }
}

data class GitEntry(val path: String, val original: String?, val index: String, val worktree: String, val conflict: Boolean) {
    val staged: Boolean get() = index != "." && index != "?"
    val unstaged: Boolean get() = worktree != "."
}

data class GitSnapshot(val root: String, val branch: String, val upstream: String?, val ahead: Int, val behind: Int,
                       val entries: List<GitEntry>, val revision: String) {
    companion object {
        fun parse(value: JSONObject): GitSnapshot {
            val status = value.getJSONObject("status")
            val entries = status.getJSONArray("entries")
            return GitSnapshot(value.getString("root"), status.getString("branch"),
                if (status.isNull("upstream")) null else status.getString("upstream"), status.getInt("ahead"), status.getInt("behind"),
                List(entries.length()) { i -> entries.getJSONObject(i).let { entry ->
                    GitEntry(entry.getString("path"), if (entry.isNull("original")) null else entry.getString("original"),
                        entry.getString("index"), entry.getString("worktree"), entry.getBoolean("conflict"))
                } }, value.getString("revision"))
        }
    }
}

enum class GitAction(val method: String) { Stage("stage"), Unstage("unstage"), Commit("commit"), Fetch("fetch"), Pull("pull"), Push("push"), Sync("sync") }
data class GitCommit(val hash: String, val shortHash: String, val subject: String, val author: String, val timestamp: Long, val refs: String)
data class GitDiffSelection(val path: String = "", val staged: Boolean = false, val commit: String = "")
data class GitProblem(val code: String, val detail: String = "") : java.io.IOException(code)
data class GitViewState(val snapshot: GitSnapshot? = null, val busy: Boolean = false, val needsRefresh: Boolean = true,
                        val error: GitProblem? = null, val completed: List<GitAction> = emptyList(),
                        val history: List<GitCommit> = emptyList(), val hasMore: Boolean = false,
                        val diff: GitDiffSelection? = null, val diffText: String? = null)

data class GitTreeRow(val path: String, val depth: Int, val entry: GitEntry? = null)

/** 仅生成可见的展示行；原始路径仍用于请求，缩进或换行不改变文件身份。 */
fun gitTree(entries: List<GitEntry>, collapsed: Set<String>): List<GitTreeRow> = buildList {
    val folders = HashSet<String>()
    entries.sortedBy { it.path }.forEach { entry ->
        val parts = entry.path.split('/')
        var parent = ""
        var hidden = false
        for (depth in 0 until parts.lastIndex) {
            parent = if (parent.isEmpty()) parts[depth] else "$parent/${parts[depth]}"
            if (folders.add(parent)) add(GitTreeRow(parent, depth))
            if (parent in collapsed) { hidden = true; break }
        }
        if (!hidden) add(GitTreeRow(entry.path, parts.lastIndex, entry))
    }
}
