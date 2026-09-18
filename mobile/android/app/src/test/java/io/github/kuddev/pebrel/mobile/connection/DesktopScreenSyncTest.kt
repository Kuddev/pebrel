package io.github.kuddev.pebrel.mobile.connection

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class DesktopScreenSyncTest {
    private fun full() = JSONObject("""{"screen_seq":1,"screen":{"version":1,"columns":1,"rows":[[["a",1,-257,-258,0]],[["b",1,-257,-258,0]]],"cursor":[0,1,1],"palette":[]}}""")
    private fun delta(base: Int = 1) = JSONObject("""{"screen_seq":2,"screen_delta":{"base":$base,"rows":[[1,[["中",1,-257,-258,0]]]],"cursor":[0,1,1],"palette":[]}}""")
    @Test fun initialSnapshotThenExactRowsAndUnchangedFrame() {
        val sync = DesktopScreenSync()
        val first = sync.apply("1:2", full())
        val updated = sync.apply("1:2", delta())
        assertTrue(updated.screenChanged)
        val rows = updated.response.getJSONObject("screen").getJSONArray("rows")
        assertEquals("a", rows.getJSONArray(0).getJSONArray(0).getString(0))
        assertEquals("中", rows.getJSONArray(1).getJSONArray(0).getString(0))
        assertEquals("b", first.response.getJSONObject("screen").getJSONArray("rows").getJSONArray(1).getJSONArray(0).getString(0))
        val same = delta(2)
        same.getJSONObject("screen_delta").put("rows", JSONArray())
        assertFalse(sync.apply("1:2", same).screenChanged)
    }
    @Test fun wrongBaselinePaneAndRowCannotCorruptTheScreen() {
        val sync = DesktopScreenSync()
        sync.apply("1:2", full())
        assertThrows(IllegalArgumentException::class.java) { sync.apply("1:2", delta(4)) }
        assertThrows(IllegalArgumentException::class.java) { sync.apply("2:2", delta()) }
        val invalid = delta()
        invalid.getJSONObject("screen_delta").getJSONArray("rows").getJSONArray(0).put(0, 2)
        assertThrows(IllegalArgumentException::class.java) { sync.apply("1:2", invalid) }
        assertEquals(1L, sync.since("1:2"))
        assertEquals(0L, sync.since("2:2"))
        sync.reset()
        assertEquals(0L, sync.since("1:2"))
        assertThrows(IllegalArgumentException::class.java) { sync.apply("1:2", delta()) }
    }
}
