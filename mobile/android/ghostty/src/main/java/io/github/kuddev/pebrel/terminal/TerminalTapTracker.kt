package io.github.kuddev.pebrel.terminal

import android.content.Context
import android.view.MotionEvent
import android.view.ViewConfiguration
import kotlin.math.abs

/** Counts completed single-pointer taps; scrolling, long presses and pinches never count. */
internal class TerminalTapTracker(context: Context) {
    private val slop = ViewConfiguration.get(context).scaledTouchSlop
    private val doubleSlop = ViewConfiguration.get(context).scaledDoubleTapSlop
    private var count = 0
    private var lastUp = -1L
    private var downX = 0f
    private var downY = 0f
    private var lastX = 0f
    private var lastY = 0f
    private var candidate = false

    fun reset() { count = 0; lastUp = -1; candidate = false }

    fun onTouch(event: MotionEvent): Int {
        if (event.pointerCount != 1) { reset(); return 0 }
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                if (lastUp < 0 || event.eventTime - lastUp > ViewConfiguration.getDoubleTapTimeout() ||
                    abs(event.x - lastX) > doubleSlop || abs(event.y - lastY) > doubleSlop) count = 0
                downX = event.x; downY = event.y; candidate = true
            }
            MotionEvent.ACTION_MOVE -> if (abs(event.x - downX) > slop || abs(event.y - downY) > slop) reset()
            MotionEvent.ACTION_CANCEL, MotionEvent.ACTION_POINTER_DOWN -> reset()
            MotionEvent.ACTION_UP -> {
                if (!candidate || event.eventTime - event.downTime >= ViewConfiguration.getLongPressTimeout()) {
                    reset(); return 0
                }
                candidate = false
                lastUp = event.eventTime; lastX = event.x; lastY = event.y
                count++
                return count.also { if (it == 3) reset() }
            }
        }
        return 0
    }
}
