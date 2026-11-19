package io.github.kuddev.pebrel.terminal

import android.graphics.Paint
import kotlin.math.floor
import kotlin.math.max

/** Same default 1.2 em row policy as desktop; Paint dimensions are already device pixels. */
internal data class TerminalGridMetrics(val width: Float, val height: Float, val baseline: Float) {
    companion object {
        fun measure(paint: Paint): TerminalGridMetrics {
            val font = paint.fontMetrics
            val height = max(1f, floor(paint.textSize * 1.2f))
            return TerminalGridMetrics(max(1f, paint.measureText("M")), height,
                (height - (font.descent - font.ascent)) / 2f - font.ascent)
        }
    }
}
