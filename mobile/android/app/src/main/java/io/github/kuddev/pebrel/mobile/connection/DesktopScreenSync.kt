package io.github.kuddev.pebrel.mobile.connection

import org.json.JSONArray
import org.json.JSONObject

data class DesktopPaneRead(val response: JSONObject, val screenChanged: Boolean = true)

/** One bounded baseline per connection. Never apply a delta to another pane or revision. */
internal class DesktopScreenSync {
    private var target = ""
    private var sequence = 0L
    private var screen: JSONObject? = null

    fun since(identity: String): Long = if (target == identity) sequence else 0
    fun reset() { target = ""; sequence = 0; screen = null }

    fun apply(identity: String, response: JSONObject): DesktopPaneRead {
        val next = response.getLong("screen_seq")
        require(next > 0)
        val full = response.optJSONObject("screen")
        val old = screen
        val updated = if (full != null) full else {
            val delta = response.getJSONObject("screen_delta")
            require(old != null && target == identity && delta.getLong("base") == sequence)
            require(next == sequence || next == sequence + 1)
            val rows = old.getJSONArray("rows")
            val patch = delta.getJSONArray("rows")
            require(patch.length() <= rows.length())
            val replacement = JSONArray()
            for (i in 0 until rows.length()) replacement.put(rows.getJSONArray(i))
            var last = -1
            for (i in 0 until patch.length()) {
                val pair = patch.getJSONArray(i)
                require(pair.length() == 2)
                val index = pair.getInt(0)
                require(index > last && index < rows.length())
                replacement.put(index, pair.getJSONArray(1))
                last = index
            }
            require(next != sequence || patch.length() == 0 &&
                delta.getJSONArray("cursor").toString() == old.getJSONArray("cursor").toString() &&
                delta.getJSONArray("palette").toString() == old.getJSONArray("palette").toString())
            JSONObject().put("version", old.getInt("version")).put("columns", old.getInt("columns"))
                .put("rows", replacement).put("cursor", delta.getJSONArray("cursor")).put("palette", delta.getJSONArray("palette"))
        }
        val changed = full != null || target != identity || next != sequence
        screen = updated
        target = identity
        sequence = next
        response.put("screen", updated)
        response.remove("screen_delta")
        return DesktopPaneRead(response, changed)
    }
}
