package io.github.kuddev.pebrel.terminal

import android.content.ClipboardManager
import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Typeface
import android.os.Handler
import android.os.Looper
import android.text.InputType
import android.view.*
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import android.widget.OverScroller
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.roundToInt

/** Hardware Canvas uses Android's shaping/fallback/glyph cache; Compose never draws cells. */
class GhosttyView(context: Context) : View(context) {
    companion object {
        private const val CURSOR_BAR = 0
        private const val CURSOR_UNDERLINE = 1
        private const val CURSOR_BLOCK = 2
        private const val MIN_FONT_SIZE = 8
        private const val MAX_FONT_SIZE = 32
        private const val CURSOR_BLINK_PERIOD_MS = 530L
    }

    private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        typeface = Typeface.MONOSPACE
        textSize = 14 * resources.displayMetrics.scaledDensity
        fontFeatureSettings = "'liga' 0, 'calt' 0"
    }
    private val fling = OverScroller(context)
    private var flingY = 0
    private var flingInputGeneration = 0
    private val blinkHandler = Handler(Looper.getMainLooper())
    private var blinkScheduled = false
    private var cursorOn = true
    private var cursorBlinkEnabled = true
    private var cursorStyle = CURSOR_BLOCK
    private var pinchZoomEnabled = true
    private var fontSize = 14
    private var onFontSizeChanged: ((Int) -> Unit)? = null
    var onZoomChanged: ((Int, Boolean) -> Unit)? = null
    private var cellWidth = 1f
    private var cellHeight = 1f
    private var baseline = 1f
    private val selection: TerminalSelection = TerminalSelection(this,
        canPaste = { session?.isReady == true },
        paste = { text -> (session?.paste(text) == true).also(::accept) },
        changed = { resetCursorBlink() },
        startScroll = ::replayPointerGesture,
    )
    internal var composingText = ""
    private var inputGeneration = 0
    private var pinchInProgress = false
    private var pinchFontSize = fontSize.toFloat()
    private var pinchChanged = false
    private val blinkTask = object : Runnable {
        override fun run() {
            blinkScheduled = false
            if (!shouldBlinkCursor()) {
                cursorOn = true
                invalidate()
                return
            }
            cursorOn = !cursorOn
            invalidate()
            scheduleCursorBlink()
        }
    }
    var session: TerminalSession? = null
        set(value) {
            if (field === value) return
            inputGeneration++
            stopScrolling()
            field?.setVisible(false)
            selection.clear()
            field = value
            updateGeometry()
            field?.setVisible(isShown && isAttachedToWindow)
            resetCursorBlink()
            invalidate()
        }
    var directInput = false
        set(value) {
            if (field == value) return
            inputGeneration++
            field = value
            isFocusable = value
            isFocusableInTouchMode = value
            if (!value) {
                clearFocus()
                composingText = ""
                context.getSystemService(InputMethodManager::class.java).hideSoftInputFromWindow(windowToken, 0)
            }
            resetCursorBlink()
        }

    init {
        setWillNotDraw(false)
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_YES
        updateMetrics()
    }
    fun setFont(typeface: Typeface, size: Float) {
        if (paint.typeface == typeface && paint.textSize == size) return
        selection.clear()
        paint.typeface = typeface
        paint.textSize = size
        fontSize = (size / resources.displayMetrics.scaledDensity).roundToInt()
            .coerceIn(MIN_FONT_SIZE, MAX_FONT_SIZE)
        updateMetrics()
        updateGeometry()
        invalidate()
    }

    /** Apply display preferences without making the renderer depend on the app settings model. */
    fun setTerminalPreferences(
        typeface: Typeface,
        fontSize: Int,
        cursorStyle: String,
        cursorBlink: Boolean,
        pinchZoom: Boolean,
        onFontSizeChanged: (Int) -> Unit,
    ) {
        val previousStyle = this.cursorStyle
        val previousBlink = this.cursorBlinkEnabled
        val previousPinch = this.pinchZoomEnabled
        val previousTypeface = paint.typeface
        val previousSize = this.fontSize
        this.cursorStyle = when (cursorStyle) {
            "bar" -> CURSOR_BAR
            "underline" -> CURSOR_UNDERLINE
            else -> CURSOR_BLOCK
        }
        this.cursorBlinkEnabled = cursorBlink
        this.pinchZoomEnabled = pinchZoom
        this.onFontSizeChanged = onFontSizeChanged
        val boundedSize = fontSize.coerceIn(MIN_FONT_SIZE, MAX_FONT_SIZE)
        val sizePx = boundedSize * resources.displayMetrics.scaledDensity
        // 缩放提示会触发 Compose 重绘；手势结束前不能用尚未保存的旧字号覆盖实时缩放。
        if (!pinchInProgress) {
            if (paint.typeface != typeface || paint.textSize != sizePx) setFont(typeface, sizePx)
            else this.fontSize = boundedSize
        }
        if (previousStyle != this.cursorStyle || previousBlink != cursorBlink || previousPinch != pinchZoom ||
            previousTypeface != paint.typeface || previousSize != this.fontSize) resetCursorBlink()
        invalidate()
    }

    private fun shouldBlinkCursor(): Boolean {
        return cursorBlinkEnabled && selection.frame == null && isAttachedToWindow && isShown &&
            visibility == VISIBLE && windowVisibility == VISIBLE && hasWindowFocus() &&
            (!directInput || hasFocus()) &&
            session?.frame?.cursorVisible == true
    }

    private fun scheduleCursorBlink() {
        if (blinkScheduled || !shouldBlinkCursor()) return
        blinkScheduled = true
        blinkHandler.postDelayed(blinkTask, CURSOR_BLINK_PERIOD_MS)
    }

    private fun resetCursorBlink() {
        blinkHandler.removeCallbacks(blinkTask)
        blinkScheduled = false
        cursorOn = true
        scheduleCursorBlink()
    }

    private fun stopCursorBlink() {
        blinkHandler.removeCallbacks(blinkTask)
        blinkScheduled = false
        cursorOn = true
    }

    private fun updateMetrics() {
        cellWidth = max(1f, paint.measureText("M"))
        val metrics = paint.fontMetrics
        cellHeight = ceil(metrics.descent - metrics.ascent + metrics.leading)
        baseline = -metrics.ascent
    }
    private fun updateGeometry() {
        if (width > 0 && height > 0) session?.updateSize(
            floor(width / cellWidth).toInt(), floor(height / cellHeight).toInt(), ceil(cellWidth).toInt(), ceil(cellHeight).toInt())
    }
    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        stopScrolling()
        selection.clear()
        updateGeometry()
    }
    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        session?.setVisible(isShown)
        resetCursorBlink()
    }
    override fun onDetachedFromWindow() {
        inputGeneration++
        composingText = ""
        stopScrolling()
        session?.setVisible(false)
        selection.clear()
        stopCursorBlink()
        super.onDetachedFromWindow()
    }
    override fun onWindowVisibilityChanged(visibility: Int) {
        super.onWindowVisibilityChanged(visibility)
        session?.setVisible(visibility == VISIBLE && isAttachedToWindow)
        if (visibility == VISIBLE) resetCursorBlink() else {
            stopScrolling()
            selection.clear()
            stopCursorBlink()
        }
    }
    override fun onVisibilityChanged(changedView: View, visibility: Int) {
        super.onVisibilityChanged(changedView, visibility)
        if (changedView !== this) return
        session?.setVisible(visibility == VISIBLE && isAttachedToWindow && windowVisibility == VISIBLE)
        if (visibility == VISIBLE) resetCursorBlink() else {
            stopScrolling()
            selection.clear()
            stopCursorBlink()
        }
    }
    override fun onWindowFocusChanged(hasWindowFocus: Boolean) {
        super.onWindowFocusChanged(hasWindowFocus)
        if (hasWindowFocus) resetCursorBlink() else { stopScrolling(); stopCursorBlink() }
    }
    override fun onFocusChanged(gainFocus: Boolean, direction: Int, previouslyFocusedRect: android.graphics.Rect?) {
        super.onFocusChanged(gainFocus, direction, previouslyFocusedRect)
        if (gainFocus) resetCursorBlink() else { stopScrolling(); stopCursorBlink() }
    }
    fun onScreenUpdated() {
        post {
            resetCursorBlink()
            postInvalidateOnAnimation()
        }
    }

    override fun onDraw(canvas: Canvas) {
        val frame = selection.frame ?: session?.frame ?: return
        // AndroidView inside Compose does not imply clipping. An unbounded
        // drawColor can erase sibling chrome recorded earlier in the display list.
        val checkpoint = canvas.save()
        canvas.clipRect(0, 0, width, height)
        paint.color = frame.background
        paint.alpha = 255
        canvas.drawRect(0f, 0f, width.toFloat(), height.toFloat(), paint)
        frame.rows.forEachIndexed { y, row -> if (row != null) drawRow(canvas, frame, row, y) }
        if (frame.cursorVisible && selection.frame == null && cursorOn) drawCursor(canvas, frame)
        canvas.restoreToCount(checkpoint)
        selection.geometry(cellWidth, cellHeight)
        selection.draw(canvas)
    }

    private fun drawCursor(canvas: Canvas, frame: TerminalFrame) {
        val left = frame.cursorX * cellWidth
        val top = frame.cursorY * cellHeight
        paint.color = frame.cursorColor
        paint.style = Paint.Style.FILL
        paint.alpha = when (cursorStyle) {
            CURSOR_BLOCK -> 92
            else -> 210
        }
        when (cursorStyle) {
            CURSOR_BAR -> canvas.drawRect(left, top, left + max(2f, resources.displayMetrics.density), top + cellHeight, paint)
            CURSOR_UNDERLINE -> {
                val thickness = max(2f, resources.displayMetrics.density)
                canvas.drawRect(left, top + cellHeight - thickness, left + cellWidth, top + cellHeight, paint)
            }
            else -> canvas.drawRect(left, top, left + cellWidth, top + cellHeight, paint)
        }
        paint.alpha = 255
        if (composingText.isNotEmpty()) {
            paint.isUnderlineText = true
            canvas.drawText(composingText, left, top + baseline, paint)
            paint.isUnderlineText = false
        }
    }

    private fun drawRow(canvas: Canvas, frame: TerminalFrame, row: TerminalRow, y: Int) {
        TerminalCellPainter.row(canvas, paint, frame, row, y, cellWidth, cellHeight, baseline)
    }

    private val gestures = GestureDetector(context, object : GestureDetector.SimpleOnGestureListener() {
        override fun onDown(event: MotionEvent) = true
        override fun onSingleTapUp(event: MotionEvent): Boolean {
            performClick()
            if (directInput) { requestFocus(); context.getSystemService(InputMethodManager::class.java).showSoftInput(this@GhosttyView, 0) }
            return true
        }
        override fun onScroll(first: MotionEvent?, current: MotionEvent, distanceX: Float, distanceY: Float): Boolean {
            scrollPixels(distanceY)
            return true
        }
        override fun onFling(first: MotionEvent?, current: MotionEvent, velocityX: Float, velocityY: Float): Boolean {
            if (selection.frame != null || pinchInProgress) return false
            val terminal = session ?: return false
            flingInputGeneration = terminal.inputGeneration
            flingY = 0
            // Finger movement and viewport movement have opposite signs.
            val limit = ViewConfiguration.get(context).scaledMaximumFlingVelocity
            fling.fling(0, 0, 0, (-velocityY).toInt().coerceIn(-limit, limit), 0, 0, -1_000_000, 1_000_000)
            postInvalidateOnAnimation()
            return true
        }
        override fun onLongPress(event: MotionEvent) {
            stopScrolling()
            selection.geometry(cellWidth, cellHeight)
            session?.frame?.let { selection.begin(it, event.x, event.y) }
        }
    })
    private val scaleGestures = ScaleGestureDetector(context, object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
        override fun onScaleBegin(detector: ScaleGestureDetector): Boolean {
            if (!pinchZoomEnabled) return false
            pinchInProgress = true
            pinchFontSize = fontSize.toFloat()
            pinchChanged = false
            onZoomChanged?.invoke(fontSize, true)
            return true
        }

        override fun onScale(detector: ScaleGestureDetector): Boolean {
            if (!pinchZoomEnabled) return false
            pinchFontSize = (pinchFontSize * detector.scaleFactor).coerceIn(MIN_FONT_SIZE.toFloat(), MAX_FONT_SIZE.toFloat())
            val next = pinchFontSize.roundToInt().coerceIn(MIN_FONT_SIZE, MAX_FONT_SIZE)
            if (next != fontSize) {
                fontSize = next
                setFont(paint.typeface ?: Typeface.MONOSPACE, fontSize * resources.displayMetrics.scaledDensity)
                pinchChanged = true
                onZoomChanged?.invoke(fontSize, true)
            }
            return true
        }

        override fun onScaleEnd(detector: ScaleGestureDetector) {
            if (pinchChanged) onFontSizeChanged?.invoke(fontSize)
            onZoomChanged?.invoke(fontSize, false)
            pinchChanged = false
        }
    })
    private var scrollRemainder = 0f

    private fun scrollPixels(distance: Float) {
        scrollRemainder += distance
        val lines = (scrollRemainder / cellHeight).toInt()
        if (lines != 0) {
            session?.scroll(lines)
            scrollRemainder -= lines * cellHeight
        }
    }

    private fun stopScrolling() {
        fling.forceFinished(true)
        scrollRemainder = 0f
    }

    override fun computeScroll() {
        super.computeScroll()
        if (fling.isFinished) return
        if (session?.inputGeneration != flingInputGeneration) { stopScrolling(); return }
        if (!fling.computeScrollOffset()) return
        val position = fling.currY
        scrollPixels((position - flingY).toFloat())
        flingY = position
        postInvalidateOnAnimation()
    }

    override fun onGenericMotionEvent(event: MotionEvent): Boolean {
        if (!pinchInProgress && event.actionMasked == MotionEvent.ACTION_SCROLL &&
            event.isFromSource(InputDevice.SOURCE_CLASS_POINTER)) {
            if (selection.frame != null) selection.clear()
            stopScrolling()
            scrollPixels(-event.getAxisValue(MotionEvent.AXIS_VSCROLL) * cellHeight * 3)
            return true
        }
        return super.onGenericMotionEvent(event)
    }

    private fun cancelPointerGesture(event: MotionEvent) {
        val cancel = MotionEvent.obtain(event)
        cancel.action = MotionEvent.ACTION_CANCEL
        gestures.onTouchEvent(cancel)
        cancel.recycle()
    }

    private fun replayPointerGesture(event: MotionEvent, downX: Float, downY: Float) {
        val down = MotionEvent.obtain(event)
        down.action = MotionEvent.ACTION_DOWN
        down.setLocation(downX, downY)
        gestures.onTouchEvent(down)
        down.recycle()
        gestures.onTouchEvent(event)
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                stopScrolling()
                parent?.requestDisallowInterceptTouchEvent(true)
            }
            MotionEvent.ACTION_CANCEL -> {
                stopScrolling()
                parent?.requestDisallowInterceptTouchEvent(false)
            }
            MotionEvent.ACTION_UP -> parent?.requestDisallowInterceptTouchEvent(false)
        }
        selection.geometry(cellWidth, cellHeight)
        if (selection.touch(event)) return true
        if (event.actionMasked == MotionEvent.ACTION_DOWN) pinchInProgress = false
        val wasPinching = pinchInProgress
        val startedByPointer = pinchZoomEnabled && event.pointerCount >= 2 && !pinchInProgress
        if (startedByPointer) {
            stopScrolling()
            pinchInProgress = true
            pinchFontSize = fontSize.toFloat()
            pinchChanged = false
            cancelPointerGesture(event)
        }
        if (pinchZoomEnabled || pinchInProgress) scaleGestures.onTouchEvent(event)
        if (!wasPinching && pinchInProgress && !startedByPointer) cancelPointerGesture(event)
        if (pinchInProgress || scaleGestures.isInProgress) {
            if (event.actionMasked == MotionEvent.ACTION_UP || event.actionMasked == MotionEvent.ACTION_CANCEL) {
                pinchInProgress = false
                resetCursorBlink()
            }
            return true
        }
        gestures.onTouchEvent(event)
        // Once DOWN is accepted, retain the whole stream through touch-slop and UP.
        // GestureDetector can return false for intermediate events before scrolling starts.
        return true
    }
    override fun performClick(): Boolean { super.performClick(); return true }

    override fun onInitializeAccessibilityNodeInfo(info: android.view.accessibility.AccessibilityNodeInfo) {
        super.onInitializeAccessibilityNodeInfo(info)
        selection.accessibility(info)
    }
    override fun performAccessibilityAction(action: Int, arguments: android.os.Bundle?): Boolean =
        selection.accessibilityAction(action) || super.performAccessibilityAction(action, arguments)

    internal fun pasteClipboard() {
        val clip = context.getSystemService(ClipboardManager::class.java).primaryClip ?: return
        if (clip.itemCount > 0) accept(session?.paste(clip.getItemAt(0).coerceToText(context).toString()) == true)
    }
    internal fun accept(result: Boolean) {
        if (result) { stopScrolling(); resetCursorBlink() } else session?.reportRejected()
    }
    override fun onCheckIsTextEditor() = directInput
    override fun onCreateInputConnection(info: EditorInfo): InputConnection? {
        if (!directInput) return null
        info.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        info.imeOptions = EditorInfo.IME_FLAG_NO_EXTRACT_UI or EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING or EditorInfo.IME_ACTION_NONE
        info.initialSelStart = 0
        info.initialSelEnd = 0
        val owner = session ?: return null
        val generation = inputGeneration
        val target = object : TerminalInputTarget {
            override fun text(text: String) = owner.sendText(text).also(::accept)
            override fun key(code: Int, modifiers: Int, action: Int, text: String, unshifted: Int) =
                owner.key(code, modifiers, action, text, unshifted).also(::accept)
            override fun paste(text: String) = owner.paste(text).also(::accept)
        }
        return TerminalInputConnection(this, target,
            { directInput && session === owner && inputGeneration == generation }, { composingText = it })
    }
    override fun onKeyDown(code: Int, event: KeyEvent): Boolean = handleKey(code, event, if (event.repeatCount > 0) 2 else 1) || super.onKeyDown(code, event)
    override fun onKeyUp(code: Int, event: KeyEvent): Boolean = handleKey(code, event, 0) || super.onKeyUp(code, event)
    private fun handleKey(code: Int, event: KeyEvent, action: Int): Boolean {
        if (selection.key(code, event, action)) return true
        if (!directInput || KeyEvent.isModifierKey(code) || code in setOf(KeyEvent.KEYCODE_BACK, KeyEvent.KEYCODE_VOLUME_UP, KeyEvent.KEYCODE_VOLUME_DOWN)) return false
        val mods = (if (event.isShiftPressed) 1 else 0) or (if (event.isCtrlPressed) 2 else 0) or (if (event.isAltPressed) 4 else 0) or (if (event.isMetaPressed) 8 else 0)
        val point = event.getUnicodeChar(event.metaState and KeyEvent.META_CTRL_MASK.inv())
        val text = if (point in 1..0x10ffff) String(Character.toChars(point)) else ""
        accept(session?.key(code, mods, action, text, event.getUnicodeChar(0)) == true)
        return true
    }
}
