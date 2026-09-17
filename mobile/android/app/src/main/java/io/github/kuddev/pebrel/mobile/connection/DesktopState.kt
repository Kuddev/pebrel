package io.github.kuddev.pebrel.mobile.connection

import org.json.JSONObject

data class DesktopPane(
    val window: Long, val id: Long, val title: String, val cwd: String,
    val task: String, val state: String, val sequence: Long,
)

/** A projection of desktop authority; never infer task completion from terminal text. */
fun parseDesktopPanes(snapshot: JSONObject): List<DesktopPane> = buildList {
    val windows = snapshot.getJSONArray("windows")
    for (w in 0 until windows.length()) {
        val window = windows.getJSONObject(w)
        val tabs = window.getJSONArray("tabs")
        for (t in 0 until tabs.length()) {
            val tab = tabs.getJSONObject(t)
            val panes = tab.getJSONArray("panes")
            for (p in 0 until panes.length()) {
                val pane = panes.getJSONObject(p)
                add(DesktopPane(window.getLong("id"), pane.getLong("id"),
                    pane.optString("title").ifBlank { tab.optString("label") }, pane.optString("cwd"),
                    if (pane.isNull("running_program")) "" else pane.optString("running_program"),
                    pane.optString("task_state", "unknown"), pane.optLong("state_change_seq")))
            }
        }
    }
}

/** Live de-dup only. This client does not advertise durable missed-event replay yet. */
class DesktopTransitions {
    private var process: Long? = null
    private val sequences = LinkedHashMap<Pair<Long, Long>, Long>()
    fun observe(snapshot: JSONObject): List<DesktopPane> {
        val id = snapshot.getLong("process_id")
        val first = process != id
        if (first) { sequences.clear(); process = id }
        val panes = parseDesktopPanes(snapshot)
        val changed = panes.filter { pane ->
            val key = pane.window to pane.id
            val previous = sequences[key]
            if (previous == null || pane.sequence > previous) sequences[key] = pane.sequence
            !first && previous != null && pane.sequence > previous &&
                pane.state in setOf("finished", "failed", "waiting_input", "attention")
        }
        sequences.keys.retainAll(panes.map { it.window to it.id }.toSet())
        return changed
    }
}
