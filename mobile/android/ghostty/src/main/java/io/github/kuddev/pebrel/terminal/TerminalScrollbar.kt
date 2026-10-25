package io.github.kuddev.pebrel.terminal

import android.graphics.Canvas
import android.graphics.Paint
import android.view.MotionEvent
import android.view.View
import kotlin.math.max

/** A viewport overlay: dragging changes position, never terminal text or PTY geometry. */
internal class TerminalScrollbar(private val view: View, private val seek: (Float) -> Unit) {
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val density get() = view.resources.displayMetrics.density
    private var fraction = 0f
    private var visible = 1f
    private var grip = 0f
    private var dragging = false
    private val inset get() = 6 * density
    private val track get() = (view.height - inset * 2).coerceAtLeast(0f)
    private val thumb get() = max(48 * density, track * visible).coerceAtMost(track)
    private val travel get() = track - thumb
    private val top get() = inset + travel * fraction

    fun update(total: Float, viewport: Float, offset: Float) {
        visible = if (total > 0) (viewport / total).coerceIn(0f, 1f) else 1f
        fraction = if (total > viewport) (offset / (total - viewport)).coerceIn(0f, 1f) else 0f
    }

    fun draw(canvas: Canvas, color: Int) {
        if (visible >= 1f || travel <= 0) return
        paint.color = color
        paint.alpha = if (dragging) 210 else 115
        val right = view.width - 2 * density
        canvas.drawRoundRect(right - 4 * density, top, right, top + thumb, 2 * density, 2 * density, paint)
    }

    fun touch(event: MotionEvent): Boolean {
        if (event.pointerCount != 1) { cancel(); return false }
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                // 只抓住边缘滑块；正文中的长按、选择和滑动继续由终端处理。
                if (visible >= 1f || travel <= 0 || event.x < view.width - 24 * density ||
                    event.y < top - 8 * density || event.y > top + thumb + 8 * density) return false
                grip = (event.y - top).coerceIn(0f, thumb)
                dragging = true
                view.parent?.requestDisallowInterceptTouchEvent(true)
                view.invalidate()
            }
            MotionEvent.ACTION_MOVE -> {
                if (!dragging) return false
                fraction = ((event.y - inset - grip) / travel).coerceIn(0f, 1f)
                seek(fraction)
                view.invalidate()
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                if (!dragging) return false
                cancel()
            }
            else -> return dragging
        }
        return true
    }

    fun cancel() {
        if (dragging) view.parent?.requestDisallowInterceptTouchEvent(false)
        dragging = false
        view.invalidate()
    }
}
