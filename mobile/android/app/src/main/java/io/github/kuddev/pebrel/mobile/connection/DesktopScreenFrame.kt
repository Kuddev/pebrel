package io.github.kuddev.pebrel.mobile.connection

import io.github.kuddev.pebrel.terminal.TerminalFrame
import io.github.kuddev.pebrel.terminal.TerminalRow
import org.json.JSONObject

/** Strict decoder: packet text cannot inject ANSI, clipboard operations or terminal input. */
internal fun decodeDesktopScreen(screen: JSONObject, theme: IntArray): TerminalFrame {
    require(theme.size == 19 && screen.getInt("version") == 1)
    val columns = screen.getInt("columns")
    val rows = screen.getJSONArray("rows")
    require(columns in 1..400 && rows.length() in 1..200 && columns * rows.length() <= 40_000)
    val palette = IntArray(269) { index ->
        val rgb = when (index) {
            in 0..15 -> theme[index + 3]
            in 16..231 -> {
                fun channel(n: Int) = if (n == 0) 0 else 55 + n * 40
                val n = index - 16
                (channel(n / 36) shl 16) or (channel(n / 6 % 6) shl 8) or channel(n % 6)
            }
            in 232..255 -> (8 + (index - 232) * 10) * 0x010101
            257 -> theme[1]
            258 -> theme[2]
            in 259..266 -> theme[index - 259 + 3]
            else -> theme[0]
        }
        rgb or 0xff000000.toInt()
    }
    val overrides = screen.getJSONArray("palette")
    require(overrides.length() <= palette.size)
    val seen = HashSet<Int>()
    for (i in 0 until overrides.length()) {
        val pair = overrides.getJSONArray(i)
        require(pair.length() == 2)
        val index = pair.getInt(0)
        val rgb = pair.getLong(1)
        require(index in palette.indices && seen.add(index) && rgb in 0..0xffffff)
        palette[index] = rgb.toInt() or 0xff000000.toInt()
    }
    fun color(value: Long): Int {
        require(value in -269..0xffffff)
        return if (value < 0) palette[(-value - 1).toInt()] else value.toInt() or 0xff000000.toInt()
    }
    var textBytes = 0
    val decoded = Array<TerminalRow?>(rows.length()) { y ->
        val source = rows.getJSONArray(y)
        require(source.length() in 1..columns)
        val text = StringBuilder()
        val cells = IntArray(columns * 6)
        var x = 0
        for (i in 0 until source.length()) {
            val cell = source.getJSONArray(i)
            require(cell.length() == 5)
            val glyph = cell.getString(0)
            val width = cell.getInt(1)
            val flags = cell.getInt(4)
            val bytes = glyph.toByteArray(Charsets.UTF_8).size
            textBytes += bytes
            require(glyph.isNotEmpty() && bytes <= 256 && textBytes <= 128 * 1024)
            require(glyph.none { it.code in 0..31 || it.code in 127..159 })
            require(width in 1..2 && x + width <= columns && flags in 0..63)
            val offset = x * 6
            cells[offset] = text.length
            cells[offset + 1] = glyph.length
            cells[offset + 2] = width
            cells[offset + 3] = color(cell.getLong(2))
            cells[offset + 4] = color(cell.getLong(3))
            cells[offset + 5] = flags
            text.append(glyph)
            x += width
        }
        require(x == columns)
        TerminalRow(text.toString(), cells)
    }
    val cursor = screen.getJSONArray("cursor")
    require(cursor.length() == 3 && cursor.getInt(2) in 0..1)
    val cx = cursor.getInt(0)
    val cy = cursor.getInt(1)
    if (cursor.getInt(2) == 1) require(cx in 0 until columns && cy in decoded.indices)
    return TerminalFrame(decoded, intArrayOf(columns, decoded.size, cx, cy, cursor.getInt(2), palette[257], palette[258], 2))
}
