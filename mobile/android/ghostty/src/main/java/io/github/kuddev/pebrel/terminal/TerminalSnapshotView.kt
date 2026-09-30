package io.github.kuddev.pebrel.terminal

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Typeface
import android.view.GestureDetector
import android.view.MotionEvent
import android.view.ScaleGestureDetector
import android.view.View
import android.view.KeyEvent
import android.text.InputType
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import kotlin.math.ceil
import kotlin.math.max
import kotlin.math.roundToInt

/** A grid mirror with optional authorized input. Gestures never resize the PC PTY. */
class TerminalSnapshotView(context: Context) : View(context) {
    private var inputGeneration = 0
    private var composingText = ""
    private var followInputCursor = false
    private var followOutput = true
    private var renderedFrame: TerminalFrame? = null
    private var projectedSource: TerminalFrame? = null
    private var projectedColumns = 0
    var wrapLines = false
        set(value) {
            if (field == value) return
            selection.clear()
            field = value
            reproject()
            offsetX = 0f
            offsetY = 0f
            if (height > 0) revealCursor(false)
            constrainOffsets()
            invalidate()
        }
    var inputTarget: TerminalInputTarget? = null
        set(value) {
            if (field === value) return
            field = value
            inputGeneration++
            composingText = ""
            isFocusable = value != null
            isFocusableInTouchMode = value != null
            val ime = context.getSystemService(InputMethodManager::class.java)
            if (value == null) {
                clearFocus()
                ime.hideSoftInputFromWindow(windowToken, 0)
            } else if (hasFocus()) ime.restartInput(this)
        }
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
    var onZoomChanged: ((Int, Boolean) -> Unit)? = null
    var pasteTarget: TerminalInputTarget? = null
    private val selection: TerminalSelection = TerminalSelection(this,
        canPaste = { (pasteTarget ?: inputTarget) != null },
        paste = { (pasteTarget ?: inputTarget)?.paste(it) == true },
        changed = { reproject(); constrainOffsets() },
        startScroll = { event, x, y ->
            val down = MotionEvent.obtain(event)
            down.action = MotionEvent.ACTION_DOWN
            down.setLocation(x, y)
            gestures.onTouchEvent(down)
            down.recycle()
            gestures.onTouchEvent(event)
        },
    )
    var frame: TerminalFrame? = null
        set(value) {
            if (field === value) return
            val follow = field == null || followOutput
            field = value
            // 选择固定在用户看到的投影帧上；退出选择后才恢复到最新输出。
            if (selection.active && value != null) { invalidate(); return }
            if (value == null) selection.clear()
            metrics()
            reproject()
            // 桌面网格包含光标下方的空白；从网格底部回跳会把真实输出顶出屏幕。
            // 首帧从顶部开始，只在当前视口装不下光标时滚动，不修改任何终端行。
            if (follow && value?.cursorVisible != true) offsetY = maxY()
            if (height > 0 && (follow || followInputCursor)) revealCursor(followInputCursor)
            constrainOffsets()
            invalidate()
        }

    fun setFont(typeface: Typeface, size: Int) {
        val pixels = size.coerceIn(8, 32) * resources.displayMetrics.scaledDensity
        if (paint.typeface == typeface && fontPixels == pixels) return
        selection.clear()
        paint.typeface = typeface
        fontPixels = pixels
        metrics()
        reproject()
        constrainOffsets()
        invalidate()
    }

    private fun metrics() {
        paint.textSize = fontPixels
        // Keep the chosen font readable. Pan across desktop columns instead of
        // compressing a wide desktop down to a few illegible phone pixels.
        paint.textSize = fontPixels * zoom
        cellWidth = max(.1f, paint.measureText("M"))
        val metrics = paint.fontMetrics
        cellHeight = ceil(metrics.descent - metrics.ascent + metrics.leading)
        baseline = -metrics.ascent
    }

    private fun reproject() {
        val source = frame
        val columns = if (wrapLines && width > 0) (width / cellWidth).toInt().coerceAtLeast(2) else 0
        if (projectedSource === source && projectedColumns == columns) return
        projectedSource = source
        projectedColumns = columns
        renderedFrame = if (source != null && columns > 0) reflowTerminal(source, columns) else source
    }

    private fun maxY() = max(0f, ((selection.frame ?: renderedFrame)?.rows?.size ?: 0) * cellHeight - height)
    private fun revealCursor(horizontal: Boolean) {
        val frame = renderedFrame?.takeIf { it.cursorVisible } ?: return
        val y = frame.cursorY * cellHeight
        if (y < offsetY) offsetY = y
        else if (y + cellHeight > offsetY + height) offsetY = y + cellHeight - height
        if (horizontal) {
            val x = frame.cursorX * cellWidth
            if (x < offsetX) offsetX = x
            else if (x + cellWidth * 2 > offsetX + width) offsetX = x + cellWidth * 2 - width
        }
    }
    private fun constrainOffsets() {
        offsetX = offsetX.coerceIn(0f, max(0f, ((selection.frame ?: renderedFrame)?.columns ?: 0) * cellWidth - width))
        offsetY = offsetY.coerceIn(0f, maxY())
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        selection.clear()
        metrics()
        reproject()
        if (followOutput && frame?.cursorVisible != true) offsetY = maxY()
        if (followOutput || followInputCursor) revealCursor(followInputCursor)
        constrainOffsets()
    }

    override fun onDraw(canvas: Canvas) {
        val frame = selection.frame ?: renderedFrame ?: return
        val checkpoint = canvas.save()
        canvas.clipRect(0, 0, width, height)
        canvas.drawColor(frame.background)
        canvas.translate(-offsetX, -offsetY)
        val first = (offsetY / cellHeight).toInt().coerceAtLeast(0)
        val last = ceil((offsetY + height) / cellHeight).toInt().coerceAtMost(frame.rows.size)
        for (y in first until last) frame.rows[y]?.let {
            TerminalCellPainter.row(canvas, paint, frame, it, y, cellWidth, cellHeight, baseline)
        }
        if (frame.cursorVisible && !selection.active) {
            paint.color = frame.cursorColor
            paint.alpha = 110
            canvas.drawRect(frame.cursorX * cellWidth, frame.cursorY * cellHeight,
                (frame.cursorX + 1) * cellWidth, (frame.cursorY + 1) * cellHeight, paint)
            paint.alpha = 255
        }
        if (composingText.isNotEmpty()) {
            paint.color = frame.cursorColor
            canvas.drawText(composingText, frame.cursorX * cellWidth, frame.cursorY * cellHeight + baseline, paint)
        }
        canvas.restoreToCount(checkpoint)
        selection.geometry(cellWidth, cellHeight, offsetX, offsetY)
        selection.draw(canvas)
    }

    private val scaling = ScaleGestureDetector(context, object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
        override fun onScaleBegin(detector: ScaleGestureDetector): Boolean {
            if (!pinchZoom) return false
            reportZoom(true)
            return true
        }
        override fun onScale(detector: ScaleGestureDetector): Boolean {
            if (!pinchZoom) return false
            val oldWidth = cellWidth
            val oldHeight = cellHeight
            zoom = (zoom * detector.scaleFactor).coerceIn(.5f, 5f)
            metrics()
            reproject()
            offsetX = (offsetX + detector.focusX) * cellWidth / oldWidth - detector.focusX
            offsetY = (offsetY + detector.focusY) * cellHeight / oldHeight - detector.focusY
            constrainOffsets()
            invalidate()
            reportZoom(true)
            return true
        }
        override fun onScaleEnd(detector: ScaleGestureDetector) { reportZoom(false) }
    })

    private fun reportZoom(gesturing: Boolean) {
        onZoomChanged?.invoke((fontPixels * zoom / resources.displayMetrics.scaledDensity).roundToInt(), gesturing)
    }

    private val gestures: GestureDetector = GestureDetector(context, object : GestureDetector.SimpleOnGestureListener() {
        override fun onDown(event: MotionEvent) = true
        override fun onSingleTapUp(event: MotionEvent): Boolean = performClick()
        override fun onDoubleTap(event: MotionEvent): Boolean {
            zoom = 1f
            metrics()
            reproject()
            offsetX = 0f
            offsetY = 0f
            followOutput = true
            if (frame?.cursorVisible != true) offsetY = maxY()
            revealCursor(false)
            invalidate()
            reportZoom(false)
            return true
        }
        override fun onScroll(first: MotionEvent?, current: MotionEvent, dx: Float, dy: Float): Boolean {
            if (multiTouch) return true
            followInputCursor = false
            offsetX += dx
            offsetY += dy
            constrainOffsets()
            followOutput = maxY() - offsetY < cellHeight * 2
            invalidate()
            return true
        }
        override fun onLongPress(event: MotionEvent) {
            if (!multiTouch) renderedFrame?.let {
                followInputCursor = false
                selection.geometry(cellWidth, cellHeight, offsetX, offsetY)
                selection.begin(it, event.x, event.y)
            }
        }
    })

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (event.actionMasked == MotionEvent.ACTION_DOWN) multiTouch = false
        if (event.pointerCount >= 2) multiTouch = true
        parent?.requestDisallowInterceptTouchEvent(true)
        selection.geometry(cellWidth, cellHeight, offsetX, offsetY)
        if (selection.touch(event)) {
            if (event.actionMasked == MotionEvent.ACTION_UP || event.actionMasked == MotionEvent.ACTION_CANCEL)
                parent?.requestDisallowInterceptTouchEvent(false)
            return true
        }
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

    override fun performClick(): Boolean {
        super.performClick()
        showKeyboard()
        return true
    }

    fun showKeyboard() {
        if (inputTarget == null) return
        followInputCursor = true
        revealCursor(true)
        constrainOffsets()
        invalidate()
        requestFocus()
        context.getSystemService(InputMethodManager::class.java).showSoftInput(this, 0)
    }

    override fun onCheckIsTextEditor() = inputTarget != null
    override fun onCreateInputConnection(info: EditorInfo): InputConnection? {
        val target = inputTarget ?: return null
        val generation = inputGeneration
        info.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        info.imeOptions = EditorInfo.IME_FLAG_NO_EXTRACT_UI or EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING or EditorInfo.IME_ACTION_NONE
        info.initialSelStart = 0
        info.initialSelEnd = 0
        return TerminalInputConnection(this, target, { inputTarget === target && inputGeneration == generation }, {
            composingText = it
            followInputCursor = true
        })
    }

    override fun onDetachedFromWindow() {
        selection.clear()
        inputGeneration++
        composingText = ""
        super.onDetachedFromWindow()
    }

    override fun onKeyDown(code: Int, event: KeyEvent): Boolean =
        handleKey(code, event, if (event.repeatCount > 0) 2 else 1) || super.onKeyDown(code, event)
    override fun onKeyUp(code: Int, event: KeyEvent): Boolean = handleKey(code, event, 0) || super.onKeyUp(code, event)
    private fun handleKey(code: Int, event: KeyEvent, action: Int): Boolean {
        if (selection.key(code, event, action)) return true
        val target = inputTarget ?: return false
        if (KeyEvent.isModifierKey(code) || code in setOf(KeyEvent.KEYCODE_BACK, KeyEvent.KEYCODE_VOLUME_UP, KeyEvent.KEYCODE_VOLUME_DOWN)) return false
        val mods = (if (event.isShiftPressed) 1 else 0) or (if (event.isCtrlPressed) 2 else 0) or
            (if (event.isAltPressed) 4 else 0) or (if (event.isMetaPressed) 8 else 0)
        val point = event.getUnicodeChar(event.metaState and KeyEvent.META_CTRL_MASK.inv())
        val text = if (point in 1..0x10ffff) String(Character.toChars(point)) else ""
        followInputCursor = true
        target.key(code, mods, action, text, event.getUnicodeChar(0))
        return true
    }

    override fun onInitializeAccessibilityNodeInfo(info: android.view.accessibility.AccessibilityNodeInfo) {
        super.onInitializeAccessibilityNodeInfo(info)
        selection.accessibility(info)
    }
    override fun performAccessibilityAction(action: Int, arguments: android.os.Bundle?): Boolean =
        selection.accessibilityAction(action) || super.performAccessibilityAction(action, arguments)
}
