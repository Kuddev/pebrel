package io.github.kuddev.pebrel.mobile.connection

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class DesktopStateTest {
    private fun snapshot(process: Long = 41, revision: Long = 8): JSONObject = JSONObject(
        """
        {
          "process_id": $process,
          "revision": $revision,
          "windows": [
            {
              "id": 10,
              "focused": false,
              "active_tab": 7,
              "tabs": [
                {
                  "index": 7,
                  "label": "build",
                  "kind": "shell",
                  "active": true,
                  "focused_pane_id": 102,
                  "panes": [
                    {"id": 101, "title": "left", "cwd": "/repo", "running_program": "cargo", "task_state": "running", "state_change_seq": 3},
                    {"id": 102, "title": "right", "cwd": "/repo", "running_program": "", "task_state": "waiting_input", "state_change_seq": 4}
                  ]
                },
                {
                  "index": 11,
                  "label": "logs",
                  "kind": "shell",
                  "active": false,
                  "panes": [
                    {"id": 103, "title": "logs", "cwd": "/var/log", "running_program": null, "task_state": "idle", "state_change_seq": 2}
                  ]
                }
              ]
            },
            {
              "id": 20,
              "focused": true,
              "active_tab": 2,
              "tabs": [
                {
                  "index": 2,
                  "label": "server",
                  "kind": "shell",
                  "active": true,
                  "panes": [
                    {"id": 201, "title": "server", "cwd": "/srv", "running_program": "node", "task_state": "attention", "state_change_seq": 9}
                  ]
                }
              ]
            }
          ]
        }
        """.trimIndent(),
    )

    @Test
    fun parsesWindowsTabsAndSplitPanesWithoutReplacingServerIndices() {
        val windows = parseDesktopWindows(snapshot())

        assertEquals(listOf(10L, 20L), windows.map { it.id })
        assertEquals(listOf(7, 11), windows[0].tabs.map { it.index })
        assertEquals(listOf(101L, 102L), windows[0].tabs[0].panes.map { it.id })
        assertEquals(listOf(7, 7), windows[0].tabs[0].panes.map { it.tabIndex })
        assertEquals(2, windows[1].tabs.single().index)
        assertTrue(windows[1].focused)
    }

    @Test
    fun primaryPaneUsesFocusedPaneAndCloseGuardUsesStableFirstPane() {
        val tab = parseDesktopWindows(snapshot()).first().tabs.first()

        assertEquals(102L, tab.primaryPane()?.id)
        assertEquals(101L, tab.closeGuardPaneId)
    }

    @Test
    fun primaryPaneFallsBackToPaneThatNeedsAttention() {
        val tab = parseDesktopWindows(snapshot()).last().tabs.single()

        assertEquals(201L, tab.primaryPane()?.id)
    }

    @Test
    fun flattenedPaneProjectionStillContainsEveryPane() {
        val panes = parseDesktopPanes(snapshot())

        assertEquals(listOf(101L, 102L, 103L, 201L), panes.map { it.id })
    }

    @Test
    fun rejectsOlderOrEqualRevisionFromSameDesktopProcess() {
        assertFalse(shouldApplyDesktopSnapshot(41, 8, snapshot(revision = 7)))
        assertFalse(shouldApplyDesktopSnapshot(41, 8, snapshot(revision = 8)))
        assertTrue(shouldApplyDesktopSnapshot(41, 8, snapshot(revision = 9)))
    }

    @Test
    fun acceptsRevisionRestartFromNewDesktopProcess() {
        assertTrue(shouldApplyDesktopSnapshot(41, 50, snapshot(process = 42, revision = 1)))
    }
}
