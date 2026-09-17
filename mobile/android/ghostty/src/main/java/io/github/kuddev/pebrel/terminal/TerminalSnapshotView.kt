package io.github.kuddev.pebrel.terminal

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Typeface
import android.view.GestureDetector
import android.view.MotionEvent
import android.view.ScaleGestureDetector
import android.view.View
import kotlin.math.ceil
import kotlin.math.max
import kotlin.math.min

/** A passive grid mirror. Phone gestures never resize another client's PTY. */
class TerminalSnapshotView(context: Context) : View(context) {
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        typeface = Typeface.MONOSPACE
        fontFeatureSettings = "'liga' 0, 'calt' 0"
    }
    private var fontPixels = 14f * resources.displayMetrics.scaledDensity
    private var zoom = 1f
    private var offsetX = 0f
    private var offsetY = 0f
    private var cellWidth = 1f
    private var cellHeight = 1f
    private var baseline = 1f
    private var multiTouch = false
    var pinchZoom = true
    var onCopyRequested: ((String) -> Unit)? = null
    var frame: TerminalFrame? = null
        set(value) {
            if (field === value) return
            val follow = field == null || maxY() - offsetY < cellHeight * 2
            field = value
            metrics()
            if (follow) offsetY = maxY()
            constrainOffsets()
            invalidate()
        }

    fun setFont(typeface: Typeface, size: Int) {
        val pixels = size.coerceIn(8, 32) * resources.displayMetrics.scaledDensity
        if (paint.typeface == typeface && fontPixels == pixels) return
        paint.typeface = typeface
        fontPixels = pixels
        metrics()
        constrainOffsets()
        invalidate()
    }

    private fun metrics() {
        paint.textSize = fontPixels
        val columns = frame?.columns ?: return
        val naturalWidth = max(1f, paint.measureText("M"))
        val fit = if (width > 0) min(1f, width / (columns * naturalWidth)) else 1f
        paint.textSize = fontPixels * fit * zoom
        cellWidth = max(.1f, paint.measureText("M"))
        val metrics = paint.fontMetrics
        cellHeight = ceil(metrics.descent - metrics.ascent + metrics.leading)
        baseline = -metrics.ascent
    }

    private fun maxY() = max(0f, (frame?.rows?.size ?: 0) * cellHeight - height)
    private fun constrainOffsets() {
        offsetX = offsetX.coerceIn(0f, max(0f, (frame?.columns ?: 0) * cellWidth - width))
        offsetY = offsetY.coerceIn(0f, maxY())
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        // View.height already contains h here; tail ownership belongs to the
        // previous viewport, before keyboard/rotation changes its height.
        val previousMaxY = max(0f, (frame?.rows?.size ?: 0) * cellHeight - oldh)
        val follow = oldh == 0 || previousMaxY - offsetY < cellHeight * 2
        metrics()
        if (follow) offsetY = maxY()
        constrainOffsets()
    }

    override fun onDraw(canvas: Canvas) {
        val frame = frame ?: return
        val checkpoint = canvas.save()
        canvas.clipRect(0, 0, width, height)
        canvas.drawColor(frame.background)
        canvas.translate(-offsetX, -offsetY)
        val first = (offsetY / cellHeight).toInt().coerceAtLeast(0)
        val last = ceil((offsetY + height) / cellHeight).toInt().coerceAtMost(frame.rows.size)
        for (y in first until last) frame.rows[y]?.let {
            TerminalCellPainter.row(canvas, paint, frame, it, y, cellWidth, cellHeight, baseline)
        }
        if (frame.cursorVisible) {
            paint.color = frame.cursorColor
            paint.alpha = 110
            canvas.drawRect(frame.cursorX * cellWidth, frame.cursorY * cellHeight,
                (frame.cursorX + 1) * cellWidth, (frame.cursorY + 1) * cellHeight, paint)
            paint.alpha = 255
        }
        canvas.restoreToCount(checkpoint)
    }

    private val scaling = ScaleGestureDetector(context, object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
        override fun onScale(detector: ScaleGestureDetector): Boolean {
            if (!pinchZoom) return false
            val oldWidth = cellWidth
            val oldHeight = cellHeight
            zoom = (zoom * detector.scaleFactor).coerceIn(.5f, 5f)
            metrics()
            offsetX = (offsetX + detector.focusX) * cellWidth / oldWidth - detector.focusX
            offsetY = (offsetY + detector.focusY) * cellHeight / oldHeight - detector.focusY
            constrainOffsets()
            invalidate()
            return true
        }
    })

    private val gestures = GestureDetector(context, object : GestureDetector.SimpleOnGestureListener() {
        override fun onDown(event: MotionEvent) = true
        override fun onSingleTapUp(event: MotionEvent): Boolean = performClick()
        override fun onDoubleTap(event: MotionEvent): Boolean {
            zoom = 1f
            metrics()
            offsetX = 0f
            offsetY = maxY()
            invalidate()
            return true
        }
        override fun onScroll(first: MotionEvent?, current: MotionEvent, dx: Float, dy: Float): Boolean {
            if (multiTouch) return true
            offsetX += dx
            offsetY += dy
            constrainOffsets()
            invalidate()
            return true
        }
        override fun onLongPress(event: MotionEvent) {
            // Copy requires an explicit action; merely holding the screen does not
            // replace the user's clipboard. Scope is the displayed snapshot.
            if (!multiTouch) onCopyRequested?.invoke(frame?.text().orEmpty())
        }
    })

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (event.actionMasked == MotionEvent.ACTION_DOWN) multiTouch = false
        if (event.pointerCount >= 2) multiTouch = true
        parent?.requestDisallowInterceptTouchEvent(true)
        if (pinchZoom) scaling.onTouchEvent(event)
        if (multiTouch) {
            val cancel = MotionEvent.obtain(event)
            cancel.action = MotionEvent.ACTION_CANCEL
            gestures.onTouchEvent(cancel)
            cancel.recycle()
        } else gestures.onTouchEvent(event)
        if (event.actionMasked in listOf(MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL)) {
            multiTouch = false
            parent?.requestDisallowInterceptTouchEvent(false)
        }
        return true
    }

    override fun performClick(): Boolean { super.performClick(); return true }
}
