package io.github.kuddev.pebrel.mobile.connection

import org.json.JSONObject

data class DesktopPane(
    val window: Long, val tabIndex: Int, val id: Long, val title: String, val cwd: String,
    val task: String, val state: String, val sequence: Long,
)

data class DesktopTab(
    val window: Long,
    val index: Int,
    val label: String,
    val kind: String,
    val active: Boolean,
    val focusedPaneId: Long?,
    val bell: Boolean,
    val panes: List<DesktopPane>,
) {
    val key: String get() = "$window:$index:${panes.firstOrNull()?.id ?: label}"
    val closeGuardPaneId: Long? get() = panes.firstOrNull()?.id
    fun primaryPane(): DesktopPane? =
        focusedPaneId?.let { focused -> panes.find { it.id == focused } }
            ?: panes.find { it.state == "waiting_input" || it.state == "attention" }
            ?: panes.firstOrNull()
}

data class DesktopWindow(
    val id: Long,
    val focused: Boolean,
    val activeTab: Int,
    val tabs: List<DesktopTab>,
)

/** A projection of desktop authority; never infer task completion from terminal text. */
fun parseDesktopWindows(snapshot: JSONObject): List<DesktopWindow> = buildList {
    val windows = snapshot.getJSONArray("windows")
    for (w in 0 until windows.length()) {
        val window = windows.getJSONObject(w)
        val windowId = window.getLong("id")
        val tabs = window.getJSONArray("tabs")
        val parsedTabs = buildList<DesktopTab> {
            for (t in 0 until tabs.length()) {
                val tab = tabs.getJSONObject(t)
                val tabIndex = tab.optInt("index", t)
                val panes = tab.getJSONArray("panes")
                val parsedPanes = buildList<DesktopPane> {
                    for (p in 0 until panes.length()) {
                        val pane = panes.getJSONObject(p)
                        add(DesktopPane(
                            windowId, tabIndex, pane.getLong("id"),
                            pane.optString("title").ifBlank { tab.optString("label") },
                            pane.optString("cwd"),
                            if (pane.isNull("running_program")) "" else pane.optString("running_program"),
                            pane.optString("task_state", "unknown"), pane.optLong("state_change_seq"),
                        ))
                    }
                }
                add(DesktopTab(
                    window = windowId,
                    index = tabIndex,
                    label = tab.optString("label").ifBlank { parsedPanes.firstOrNull()?.title.orEmpty() },
                    kind = tab.optString("kind", "shell"),
                    active = tab.optBoolean("active", tabIndex == window.optInt("active_tab", 0)),
                    focusedPaneId = tab.optLong("focused_pane_id").takeIf { !tab.isNull("focused_pane_id") && it > 0 },
                    bell = tab.optBoolean("bell"),
                    panes = parsedPanes,
                ))
            }
        }
        add(DesktopWindow(windowId, window.optBoolean("focused"), window.optInt("active_tab"), parsedTabs))
    }
}

fun parseDesktopPanes(snapshot: JSONObject): List<DesktopPane> =
    parseDesktopWindows(snapshot).flatMap { window -> window.tabs.flatMap(DesktopTab::panes) }

/** Snapshot responses and subscription events can cross; never let an older revision resurrect a tab. */
fun shouldApplyDesktopSnapshot(currentProcess: Long, currentRevision: Long, snapshot: JSONObject): Boolean {
    val process = snapshot.optLong("process_id", -1L)
    val revision = snapshot.optLong("revision", 0L)
    return currentProcess < 0 || process != currentProcess || revision > currentRevision
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
