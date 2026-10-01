package io.github.kuddev.pebrel.terminal

import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Path
import android.graphics.Rect
import android.graphics.Typeface
import kotlin.math.max
import kotlin.math.min

/** Shared by live terminal sessions and desktop grid mirrors. Never lays out paragraphs. */
internal object TerminalCellPainter {
    private val emojiPaint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val emojiBounds = Rect()

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
            val pause = end > start && row.text[start] == '\u23f8'
            val textPause = pause && end > start + 1 && row.text[start + 1] == '\ufe0e'
            val colorPause = pause && !textPause
            // 裸暂停符号通常只占一列，正方形 Emoji 会被压成半行高。仅借用同底色的
            // 紧邻空格绘制完整按钮；不改原文/选区坐标，也绝不覆盖相邻正文或修饰过的空格。
            if (colorPause && width == 1 && next < frame.columns) {
                val n = next * 6
                if (cells[n + 2] == 1 && cells[n + 1] == 1 && row.text[cells[n]] == ' ' &&
                    cells[n + 4] == cells[index + 4] && cells[n + 5] == 0) next++
            }
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
                val emoji = colorPause && pauseEmoji(canvas, paint, left, top, (next - x) * cellWidth, cellHeight)
                val procedural = emoji || ((end - start == 1 || textPause) && TerminalGlyphs.draw(
                    canvas, paint, row.text[start], left, top, width * cellWidth, cellHeight))
                if (!procedural) {
                    val checkpoint = canvas.save()
                    canvas.clipRect(left, top, next * cellWidth, top + cellHeight)
                    // Android 回退字体可能把终端的一列符号画成两列；缩放到原单元格，
                    // 保留完整字形和邻字边界。ASCII 批次仍走原来的零测量路径。
                    if (end - start != next - x || row.text[start].code !in 32..126) {
                        val advance = paint.measureText(row.text, start, end)
                        val allocated = (next - x) * cellWidth
                        if (advance > allocated) canvas.scale(allocated / advance, 1f, left, top)
                    }
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

    private fun pauseEmoji(canvas: Canvas, source: Paint, x: Float, y: Float, width: Float, height: Float): Boolean {
        // 桌面将暂停符号显示为彩色按钮；保留系统 Emoji 的背景，不用两条线替代它。
        // 显式 FE0E 文本形式仍走普通字体。只改变绘制，不改变终端原文和复制内容。
        val text = "\u23f8\ufe0f"
        emojiPaint.set(source)
        emojiPaint.typeface = Typeface.DEFAULT
        emojiPaint.isFakeBoldText = false
        emojiPaint.textSkewX = 0f
        emojiPaint.isUnderlineText = false
        emojiPaint.isStrikeThruText = false
        emojiPaint.getTextBounds(text, 0, text.length, emojiBounds)
        if (emojiBounds.isEmpty) return false
        val scale = min(width / emojiBounds.width(), height / emojiBounds.height())
        val checkpoint = canvas.save()
        canvas.clipRect(x, y, x + width, y + height)
        canvas.translate(x + (width - emojiBounds.width() * scale) / 2 - emojiBounds.left * scale,
            y + (height - emojiBounds.height() * scale) / 2 - emojiBounds.top * scale)
        canvas.scale(scale, scale)
        canvas.drawText(text, 0f, 0f, emojiPaint)
        canvas.restoreToCount(checkpoint)
        return true
    }
}

/** Render terminal geometry and TUI mode symbols without depending on font coverage. */
internal object TerminalGlyphs {
    fun draw(canvas: Canvas, paint: Paint, character: Char, x: Float, y: Float, w: Float, h: Float): Boolean {
        val code = character.code
        if (code !in 0x2580..0x259f && code !in BOX_ARMS && code != 0x23f5 && code != 0x23f8) return false
        val antialias = paint.isAntiAlias
        val alpha = paint.alpha
        paint.isAntiAlias = false
        fun rect(left: Float, top: Float, right: Float, bottom: Float) {
            canvas.drawRect(x + left * w, y + top * h, x + right * w, y + bottom * h, paint)
        }
        when (code) {
            0x23f8 -> {
                rect(.2f, .25f, .4f, .75f)
                rect(.6f, .25f, .8f, .75f)
            }
            0x23f5 -> {
                val checkpoint = canvas.save()
                canvas.translate(x, y)
                canvas.scale(w, h)
                paint.isAntiAlias = true
                canvas.drawPath(PLAY, paint)
                canvas.restoreToCount(checkpoint)
            }
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
    private val PLAY = Path().apply {
        moveTo(.2f, .25f)
        lineTo(.8f, .5f)
        lineTo(.2f, .75f)
        close()
    }
    private val BOX_ARMS = mapOf(0x2500 to 3, 0x2502 to 12, 0x250c to 10, 0x2510 to 9,
        0x2514 to 6, 0x2518 to 5, 0x251c to 14, 0x2524 to 13, 0x252c to 11, 0x2534 to 7, 0x253c to 15)
}
