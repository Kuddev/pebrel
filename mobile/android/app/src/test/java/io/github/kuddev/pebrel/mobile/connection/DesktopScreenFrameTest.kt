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
class DesktopScreenFrameTest {
    private val theme = intArrayOf(0xffeeeeee.toInt(), 0xff101010.toInt(), 0xffffffff.toInt()) +
        IntArray(16) { 0xff000000.toInt() or (it * 0x10101) }
    private fun screen() = JSONObject("""{"version":1,"columns":4,"rows":[
        [["中",2,14251863,-258,0],["▛",1,-2,25,1],[" ",1,-257,-26,0]]
        ],"cursor":[3,0,1],"palette":[[1,16711680]]}""")

    @Test fun desktopPaletteWinsOverOppositePhoneTheme() {
        val snapshot = screen().put("palette", JSONArray("[[256,15787730],[257,660510],[258,16777215],[1,16711680]]"))
        val dark = decodeDesktopScreen(snapshot, theme)
        val light = decodeDesktopScreen(snapshot, IntArray(19) { 0xffffffff.toInt() }.apply { this[0] = 0xff101010.toInt() })
        assertEquals(0xff0a141e.toInt(), light.background)
        assertEquals(dark.background, light.background)
        assertEquals(dark.rows[0]!!.cells[21], light.rows[0]!!.cells[21])
    }

    @Test fun preservesCellWidthsForegroundBackgroundPaletteAndCursor() {
        val frame = decodeDesktopScreen(screen(), theme)
        val row = frame.rows[0]!!
        assertEquals(4, frame.columns)
        assertEquals(2, row.cells[2])
        assertEquals(0, row.cells[8]) // second half of wide glyph is a spacer
        assertEquals(0xffd97757.toInt(), row.cells[3])
        assertEquals(0xffff0000.toInt(), row.cells[15])
        assertEquals(0xff000019.toInt(), row.cells[16])
        assertEquals(0xff005faf.toInt(), row.cells[22])
        assertTrue(frame.cursorVisible)
        assertEquals(3, frame.cursorX)
    }

    @Test fun rejectsMalformedRowsAndTerminalControlText() {
        val tooWide = screen().put("columns", 3)
        assertThrows(IllegalArgumentException::class.java) { decodeDesktopScreen(tooWide, theme) }
        val control = screen()
        control.getJSONArray("rows").getJSONArray(0).getJSONArray(0).put(0, "\u001b]52;clipboard")
        assertThrows(IllegalArgumentException::class.java) { decodeDesktopScreen(control, theme) }
        val invalidPalette = screen().put("palette", JSONArray("[[269,1]]"))
        assertThrows(IllegalArgumentException::class.java) { decodeDesktopScreen(invalidPalette, theme) }
    }

    @Test fun rejectsNewVersionsAndOutOfBoundsCursor() {
        assertThrows(IllegalArgumentException::class.java) { decodeDesktopScreen(screen().put("version", 2), theme) }
        assertThrows(IllegalArgumentException::class.java) {
            decodeDesktopScreen(screen().put("cursor", JSONArray("[4,0,1]")), theme)
        }
    }
}
