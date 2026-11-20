package io.github.kuddev.pebrel.terminal

/** 手机阅读投影：仅重排不可变单元格，不回写桌面 PTY，不重新解释 ANSI。 */
internal fun reflowTerminal(source: TerminalFrame, columns: Int): TerminalFrame {
    require(columns >= 2)
    if (columns >= source.columns) return source
    val result = ArrayList<TerminalRow?>()
    val wraps = ArrayList<Boolean>()
    val sourceOffsets = ArrayList<Int>()
    var rowSourceOffset = 0
    var text = StringBuilder()
    var cells = IntArray(columns * 6)
    var x = 0
    var cursorX = 0
    var cursorY = 0

    fun finish(wrapped: Boolean = false) {
        sourceOffsets += rowSourceOffset
        result += TerminalRow(text.toString(), cells)
        wraps += wrapped
        text = StringBuilder()
        cells = IntArray(columns * 6)
        x = 0
    }

    source.rows.forEachIndexed { y, row ->
        if (x == 0) rowSourceOffset = y * source.columns
        val wrapped = source.wrapped?.getOrNull(y) == true
        var extent = if (wrapped) source.columns else 0
        if (row != null && !wrapped) {
            var column = 0
            while (column < source.columns) {
                val offset = column * 6
                val width = row.cells[offset + 2]
                if (width == 0) { column++; continue }
                val start = row.cells[offset]
                val end = start + row.cells[offset + 1]
                if (row.cells[offset + 4] != source.background || row.cells[offset + 5] != 0 ||
                    (start until end).any { row.text[it] != ' ' }) extent = column + width
                column += width
            }
        }
        if (source.cursorVisible && y == source.cursorY) extent = maxOf(extent, source.cursorX + 1)
        var column = 0
        while (column < extent) {
            val offset = column * 6
            val width = row?.cells?.get(offset + 2) ?: 0
            if (width == 0) { column++; continue }
            if (x + width > columns) finish(wrapped = true)
            if (x == 0) rowSourceOffset = y * source.columns + column
            if (source.cursorVisible && y == source.cursorY && source.cursorX in column until column + width) {
                cursorX = x + source.cursorX - column
                cursorY = result.size
            }
            val destination = x * 6
            cells[destination] = text.length
            for (field in 1..5) cells[destination + field] = row!!.cells[offset + field]
            text.append(row!!.text, row.cells[offset], row.cells[offset] + row.cells[offset + 1])
            x += width
            column += width
        }
        // 真换行保留；电脑列宽造成的软换行先连接，再按手机列宽断行。
        if (!wrapped || y == source.rows.lastIndex) finish()
    }
    if (result.isEmpty()) finish()
    val meta = source.meta.copyOf()
    meta[0] = columns
    meta[1] = result.size
    meta[2] = cursorX
    meta[3] = cursorY
    return TerminalFrame(result.toTypedArray(), meta, wraps.toBooleanArray(), source.history, sourceOffsets.toIntArray())
}
