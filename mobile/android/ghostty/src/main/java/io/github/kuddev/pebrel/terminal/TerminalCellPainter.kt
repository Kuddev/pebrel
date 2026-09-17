package io.github.kuddev.pebrel.terminal

import android.graphics.Canvas
import android.graphics.Paint
import kotlin.math.max

/** Shared by live Ghostty terminals and desktop grid mirrors. Never lays out paragraphs. */
internal object TerminalCellPainter {
    fun row(canvas: Canvas, paint: Paint, frame: TerminalFrame, row: TerminalRow, y: Int,
            cellWidth: Float, cellHeight: Float, baseline: Float) {
        val cells = row.cells
        var x = 0
        while (x < frame.columns) {
            val index = x * 6
            val width = cells[index + 2]
            if (width == 0) { x++; continue }
            val start = cells[index]
            var end = start + cells[index + 1]
            var next = x + width
            val flags = cells[index + 5]
            if (width == 1 && end - start == 1 && row.text[start].code in 32..126) {
                while (next < frame.columns) {
                    val n = next * 6
                    if (cells[n + 2] != 1 || cells[n + 1] != 1 || cells[n] != end ||
                        row.text[end].code !in 32..126 || cells[n + 3] != cells[index + 3] ||
                        cells[n + 4] != cells[index + 4] || cells[n + 5] != flags) break
                    end++
                    next++
                }
            }
            val left = x * cellWidth
            val top = y * cellHeight
            paint.color = cells[index + 4]
            paint.alpha = 255
            paint.style = Paint.Style.FILL
            canvas.drawRect(left, top, next * cellWidth, top + cellHeight, paint)
            if (flags and 32 == 0 && end > start) {
                paint.color = cells[index + 3]
                paint.alpha = if (flags and 16 != 0) 150 else 255
                paint.isFakeBoldText = flags and 1 != 0
                paint.textSkewX = if (flags and 2 != 0) -.2f else 0f
                paint.isUnderlineText = flags and 4 != 0
                paint.isStrikeThruText = flags and 8 != 0
                val procedural = end - start == 1 && TerminalGlyphs.draw(
                    canvas, paint, row.text[start], left, top, width * cellWidth, cellHeight)
                if (!procedural) {
                    // Font fallback and CJK glyph advances must not escape the
                    // cell(s) allocated by the terminal's Unicode width rules.
                    val checkpoint = canvas.save()
                    canvas.clipRect(left, top, next * cellWidth, top + cellHeight)
                    canvas.drawTextRun(row.text, start, end, start, end,
                        left, top + baseline, false, paint)
                    canvas.restoreToCount(checkpoint)
                }
            }
            x = next
        }
        paint.isFakeBoldText = false
        paint.textSkewX = 0f
        paint.isUnderlineText = false
        paint.isStrikeThruText = false
        paint.alpha = 255
    }
}

/** Blocks/box lines are cell geometry, not font artwork (including Claude's mascot). */
internal object TerminalGlyphs {
    fun draw(canvas: Canvas, paint: Paint, character: Char, x: Float, y: Float, w: Float, h: Float): Boolean {
        val code = character.code
        if (code !in 0x2580..0x259f && code !in BOX_ARMS) return false
        val antialias = paint.isAntiAlias
        val alpha = paint.alpha
        paint.isAntiAlias = false
        fun rect(left: Float, top: Float, right: Float, bottom: Float) {
            canvas.drawRect(x + left * w, y + top * h, x + right * w, y + bottom * h, paint)
        }
        when (code) {
            0x2580 -> rect(0f, 0f, 1f, .5f)
            in 0x2581..0x2588 -> rect(0f, 1f - (code - 0x2580) / 8f, 1f, 1f)
            in 0x2589..0x258f -> rect(0f, 0f, (0x2590 - code) / 8f, 1f)
            0x2590 -> rect(.5f, 0f, 1f, 1f)
            in 0x2591..0x2593 -> {
                paint.alpha = alpha * (code - 0x2590) / 4
                rect(0f, 0f, 1f, 1f)
            }
            0x2594 -> rect(0f, 0f, 1f, .125f)
            0x2595 -> rect(.875f, 0f, 1f, 1f)
            in 0x2596..0x259f -> {
                val mask = QUADRANTS[code - 0x2596]
                if (mask and 1 != 0) rect(0f, 0f, .5f, .5f)
                if (mask and 2 != 0) rect(.5f, 0f, 1f, .5f)
                if (mask and 4 != 0) rect(0f, .5f, .5f, 1f)
                if (mask and 8 != 0) rect(.5f, .5f, 1f, 1f)
            }
            else -> {
                val arms = BOX_ARMS.getValue(code)
                val thickness = max(1f, w / 9f)
                val cx = x + w / 2f
                val cy = y + h / 2f
                val half = thickness / 2f
                if (arms and 1 != 0) canvas.drawRect(x, cy - half, cx + half, cy + half, paint)
                if (arms and 2 != 0) canvas.drawRect(cx - half, cy - half, x + w, cy + half, paint)
                if (arms and 4 != 0) canvas.drawRect(cx - half, y, cx + half, cy + half, paint)
                if (arms and 8 != 0) canvas.drawRect(cx - half, cy - half, cx + half, y + h, paint)
            }
        }
        paint.isAntiAlias = antialias
        paint.alpha = alpha
        return true
    }

    private val QUADRANTS = intArrayOf(4, 8, 1, 13, 9, 7, 11, 2, 6, 14)
    private val BOX_ARMS = mapOf(0x2500 to 3, 0x2502 to 12, 0x250c to 10, 0x2510 to 9,
        0x2514 to 6, 0x2518 to 5, 0x251c to 14, 0x2524 to 13, 0x252c to 11, 0x2534 to 7, 0x253c to 15)
}
