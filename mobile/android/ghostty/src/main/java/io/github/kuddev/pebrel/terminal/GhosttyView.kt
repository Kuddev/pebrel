package io.github.kuddev.pebrel.terminal

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Rect
import android.graphics.Typeface
import android.os.Handler
import android.os.Looper
import android.text.InputType
import android.view.*
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import android.widget.OverScroller
import kotlin.math.abs
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min
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
        private const val CELL_FIELDS = 6
    }

    private data class SelectionPoint(val row: Int, val column: Int) : Comparable<SelectionPoint> {
        override fun compareTo(other: SelectionPoint): Int =
            if (row != other.row) row.compareTo(other.row) else column.compareTo(other.column)
    }

    private enum class SelectionHandle { START, END }

    private enum class SelectionClass { WORD, SPACE, OTHER }

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
    private var cellWidth = 1f
    private var cellHeight = 1f
    private var baseline = 1f
    private var selectedFrame: TerminalFrame? = null
    private var anchor = SelectionPoint(0, 0)
    private var extent = SelectionPoint(0, 0)
    private var draggedHandle: SelectionHandle? = null
    private var selectionTouchOnHandle = false
    private var selectionTouchDecided = false
    private var selectionTouchX = 0f
    private var selectionTouchY = 0f
    private var actionMode: ActionMode? = null
    internal var composingText = ""
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
            stopScrolling()
            field?.setVisible(false)
            clearSelection()
            field = value
            updateGeometry()
            field?.setVisible(isShown && isAttachedToWindow)
            resetCursorBlink()
            invalidate()
        }
    var directInput = false
        set(value) {
            if (field == value) return
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
        clearSelection()
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
        if (paint.typeface != typeface || paint.textSize != sizePx) setFont(typeface, sizePx)
        else this.fontSize = boundedSize
        if (previousStyle != this.cursorStyle || previousBlink != cursorBlink || previousPinch != pinchZoom ||
            previousTypeface != paint.typeface || previousSize != this.fontSize) resetCursorBlink()
        invalidate()
    }

    private fun shouldBlinkCursor(): Boolean {
        return cursorBlinkEnabled && selectedFrame == null && isAttachedToWindow && isShown &&
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
        clearSelection()
        updateGeometry()
    }
    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        session?.setVisible(isShown)
        resetCursorBlink()
    }
    override fun onDetachedFromWindow() {
        stopScrolling()
        session?.setVisible(false)
        clearSelection()
        stopCursorBlink()
        super.onDetachedFromWindow()
    }
    override fun onWindowVisibilityChanged(visibility: Int) {
        super.onWindowVisibilityChanged(visibility)
        session?.setVisible(visibility == VISIBLE && isAttachedToWindow)
        if (visibility == VISIBLE) resetCursorBlink() else {
            stopScrolling()
            clearSelection()
            stopCursorBlink()
        }
    }
    override fun onVisibilityChanged(changedView: View, visibility: Int) {
        super.onVisibilityChanged(changedView, visibility)
        if (changedView !== this) return
        session?.setVisible(visibility == VISIBLE && isAttachedToWindow && windowVisibility == VISIBLE)
        if (visibility == VISIBLE) resetCursorBlink() else {
            stopScrolling()
            clearSelection()
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
        val frame = selectedFrame ?: session?.frame ?: return
        // AndroidView inside Compose does not imply clipping. An unbounded
        // drawColor can erase sibling chrome recorded earlier in the display list.
        val checkpoint = canvas.save()
        canvas.clipRect(0, 0, width, height)
        paint.color = frame.background
        paint.alpha = 255
        canvas.drawRect(0f, 0f, width.toFloat(), height.toFloat(), paint)
        frame.rows.forEachIndexed { y, row -> if (row != null) drawRow(canvas, frame, row, y) }
        if (selectedFrame != null) drawSelectionHandles(canvas, frame)
        if (frame.cursorVisible && selectedFrame == null && cursorOn) drawCursor(canvas, frame)
        canvas.restoreToCount(checkpoint)
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
        selectionRangeForRow(frame, y)?.let { range ->
            paint.color = frame.cursorColor
            paint.alpha = 70
            canvas.drawRect(range.first * cellWidth, y * cellHeight,
                (range.last + 1) * cellWidth, (y + 1) * cellHeight, paint)
            paint.alpha = 255
        }
    }

    private fun selectionRangeForRow(frame: TerminalFrame, row: Int): IntRange? {
        val low = if (anchor <= extent) anchor else extent
        val high = if (anchor <= extent) extent else anchor
        if (row < low.row || row > high.row) return null
        val from = (if (row == low.row) low.column else 0).coerceIn(0, frame.columns)
        val to = (if (row == high.row) high.column else frame.columns).coerceIn(from, frame.columns)
        return if (to > from) from until to else null
    }

    private fun drawSelectionHandles(canvas: Canvas, frame: TerminalFrame) {
        // Keep logical endpoint identity when the handles cross. Sorting here
        // would make a touch on the left handle move the wrong endpoint.
        drawSelectionHandle(canvas, frame, anchor)
        drawSelectionHandle(canvas, frame, extent)
    }

    private fun drawSelectionHandle(canvas: Canvas, frame: TerminalFrame, point: SelectionPoint) {
        val density = resources.displayMetrics.density
        val radius = max(5f * density, 5f)
        val x = selectionHandleX(point, radius)
        val y = selectionHandleY(point, radius)
        paint.style = Paint.Style.FILL
        paint.color = frame.cursorColor
        paint.alpha = 220
        canvas.drawCircle(x, y, radius, paint)
        paint.alpha = 255
    }

    private fun selectionHandleX(point: SelectionPoint, radius: Float): Float =
        clampHandleCoordinate(point.column * cellWidth, width.toFloat(), radius)

    private fun selectionHandleY(point: SelectionPoint, radius: Float): Float =
        clampHandleCoordinate(
            ((point.row + 1) * cellHeight).coerceIn(0f, height.toFloat()), height.toFloat(), radius)

    private fun selectionContentBounds(): Rect {
        val density = resources.displayMetrics.density
        val radius = max(5f * density, 5f)
        val left = min(selectionHandleX(anchor, radius), selectionHandleX(extent, radius)) - radius
        val right = max(selectionHandleX(anchor, radius), selectionHandleX(extent, radius)) + radius
        val top = min(selectionHandleY(anchor, radius), selectionHandleY(extent, radius)) - radius
        val bottom = max(selectionHandleY(anchor, radius), selectionHandleY(extent, radius)) + radius
        val leftInt = floor(left).toInt().coerceIn(0, width)
        val topInt = floor(top).toInt().coerceIn(0, height)
        var rightInt = ceil(right).toInt().coerceIn(0, width)
        var bottomInt = ceil(bottom).toInt().coerceIn(0, height)
        if (rightInt <= leftInt) rightInt = (leftInt + 1).coerceAtMost(width)
        if (bottomInt <= topInt) bottomInt = (topInt + 1).coerceAtMost(height)
        return Rect(leftInt, topInt, rightInt, bottomInt)
    }

    private fun clampHandleCoordinate(value: Float, size: Float, radius: Float): Float {
        if (size <= 0f) return 0f
        if (size <= radius * 2f) return size / 2f
        return value.coerceIn(radius, size - radius)
    }

    private fun wordSelection(frame: TerminalFrame, rowIndex: Int, x: Float): Pair<SelectionPoint, SelectionPoint> {
        val row = frame.rows.getOrNull(rowIndex)
        val columns = row?.let { min(frame.columns, it.cells.size / CELL_FIELDS) } ?: frame.columns
        if (row == null || columns == 0) {
            val column = (x / cellWidth).roundToInt().coerceIn(0, frame.columns)
            return SelectionPoint(rowIndex, column) to SelectionPoint(rowIndex, (column + 1).coerceAtMost(frame.columns))
        }
        val cell = leadingCell(row, floor(x / cellWidth).toInt().coerceIn(0, columns - 1))
        val kind = selectionClass(row, cell)
        if (kind == SelectionClass.OTHER) {
            val end = (cell + cellSpan(row, cell)).coerceAtMost(columns)
            return SelectionPoint(rowIndex, cell) to SelectionPoint(rowIndex, end)
        }
        var first = cell
        while (first > 0) {
            val previous = previousCell(row, first)
            if (selectionClass(row, previous) != kind) break
            first = previous
        }
        var end = (cell + cellSpan(row, cell)).coerceAtMost(columns)
        while (end < columns) {
            val next = nextCell(row, end, columns)
            if (next >= columns || selectionClass(row, next) != kind) break
            end = (next + cellSpan(row, next)).coerceAtMost(columns)
        }
        return SelectionPoint(rowIndex, first) to SelectionPoint(rowIndex, end)
    }

    private fun selectionPointAt(frame: TerminalFrame, x: Float, y: Float, handle: SelectionHandle? = null): SelectionPoint {
        if (frame.rows.isEmpty()) return SelectionPoint(0, 0)
        val row = (ceil(y / cellHeight).toInt() - 1).coerceIn(0, frame.rows.lastIndex)
        val rowData = frame.rows[row]
        var column = if (rowData == null) {
            (x / cellWidth).roundToInt().coerceIn(0, frame.columns)
        } else boundaryAt(rowData, frame.columns, x)
        if (handle != null) {
            val endpoint = if (handle == SelectionHandle.START) anchor else extent
            val visualRadius = max(5f * resources.displayMetrics.density, 5f)
            val logicalX = endpoint.column * cellWidth
            val handleX = selectionHandleX(endpoint, visualRadius)
            if (logicalX != handleX && abs(x - handleX) <= visualRadius) column = endpoint.column
        }
        return SelectionPoint(row, column)
    }

    private fun handleAt(x: Float, y: Float): SelectionHandle? {
        val density = resources.displayMetrics.density
        val hitRadiusX = max(22f * density, 22f)
        val visualRadius = max(5f * density, 5f)
        // Keep the target generous along the handle row, but leave the body
        // of a terminal row available for starting a scroll gesture.
        val hitRadiusY = max(12f * density, 12f)
        var result: SelectionHandle? = null
        var distance = Float.MAX_VALUE
        fun consider(handle: SelectionHandle, point: SelectionPoint) {
            val centerX = selectionHandleX(point, visualRadius)
            val centerY = selectionHandleY(point, visualRadius)
            val dx = x - centerX
            val dy = y - centerY
            val normalized = dx * dx / (hitRadiusX * hitRadiusX) + dy * dy / (hitRadiusY * hitRadiusY)
            if (normalized <= 1f && normalized < distance) {
                result = handle
                distance = normalized
            }
        }
        // Use the logical endpoints, rather than the sorted range, so a
        // crossed selection still lets either visible handle be dragged.
        consider(SelectionHandle.START, anchor)
        consider(SelectionHandle.END, extent)
        return result
    }

    private fun isNearVisualHandle(x: Float, y: Float, point: SelectionPoint): Boolean {
        val density = resources.displayMetrics.density
        val visualRadius = max(5f * density, 5f)
        val nearRadius = max(10f * density, visualRadius * 2f)
        val dx = x - selectionHandleX(point, visualRadius)
        val dy = y - selectionHandleY(point, visualRadius)
        return dx * dx + dy * dy <= nearRadius * nearRadius
    }

    private fun boundaryAt(row: TerminalRow, frameColumns: Int, x: Float): Int {
        val columns = min(frameColumns, row.cells.size / CELL_FIELDS)
        if (columns == 0) return (x / cellWidth).roundToInt().coerceIn(0, frameColumns)
        val candidate = (x / cellWidth).roundToInt().coerceIn(0, frameColumns)
        if (candidate !in 1 until columns || rawCellWidth(row, candidate) != 0) return candidate
        val first = leadingCell(row, candidate)
        val before = first * cellWidth
        val after = (first + cellSpan(row, first)).coerceAtMost(frameColumns) * cellWidth
        return if (x - before < after - x) first else (first + cellSpan(row, first)).coerceAtMost(frameColumns)
    }

    private fun selectionClass(row: TerminalRow, column: Int): SelectionClass {
        val text = cellText(row, leadingCell(row, column))
        if (text.isEmpty()) return SelectionClass.OTHER
        val codePoint = text.codePointAt(0)
        return when {
            Character.isLetterOrDigit(codePoint) || codePoint == '_'.code || isMark(codePoint) -> SelectionClass.WORD
            Character.isWhitespace(codePoint) -> SelectionClass.SPACE
            else -> SelectionClass.OTHER
        }
    }

    private fun isMark(codePoint: Int): Boolean = when (Character.getType(codePoint)) {
        Character.NON_SPACING_MARK.toInt(), Character.COMBINING_SPACING_MARK.toInt(),
        Character.ENCLOSING_MARK.toInt() -> true
        else -> false
    }

    private fun selectedRowText(row: TerminalRow?, from: Int, to: Int, frameColumns: Int): String {
        if (row == null || to <= from) return ""
        val columns = min(frameColumns, row.cells.size / CELL_FIELDS)
        val startColumn = from.coerceIn(0, columns)
        val endColumn = to.coerceIn(startColumn, columns)
        val result = StringBuilder()
        var column = startColumn
        while (column < endColumn) {
            val width = rawCellWidth(row, column)
            if (width == 0) {
                column++
                continue
            }
            result.append(cellText(row, column))
            column += width.coerceAtLeast(1)
        }
        return result.toString().trimEnd()
    }

    private fun cellText(row: TerminalRow, column: Int): String {
        val index = column * CELL_FIELDS
        if (column < 0 || index < 0 || index + 1 >= row.cells.size) return ""
        val start = row.cells[index].coerceIn(0, row.text.length)
        val end = (start + row.cells[index + 1].coerceAtLeast(0)).coerceIn(start, row.text.length)
        return if (end > start) row.text.substring(start, end) else ""
    }

    private fun rawCellWidth(row: TerminalRow, column: Int): Int {
        val index = column * CELL_FIELDS + 2
        return if (column >= 0 && index in row.cells.indices) row.cells[index].coerceIn(0, 2) else 0
    }

    private fun cellSpan(row: TerminalRow, column: Int): Int = rawCellWidth(row, column).let { if (it == 2) 2 else 1 }

    private fun leadingCell(row: TerminalRow, column: Int): Int {
        var result = column.coerceIn(0, (row.cells.size / CELL_FIELDS - 1).coerceAtLeast(0))
        while (result > 0 && rawCellWidth(row, result) == 0) result--
        return result
    }

    private fun previousCell(row: TerminalRow, column: Int): Int {
        var result = (column - 1).coerceAtLeast(0)
        while (result > 0 && rawCellWidth(row, result) == 0) result--
        return result
    }

    private fun nextCell(row: TerminalRow, column: Int, columns: Int): Int {
        var result = column.coerceAtLeast(0)
        while (result < columns && rawCellWidth(row, result) == 0) result++
        return result
    }

    private fun clearSelection() {
        val mode = actionMode
        actionMode = null
        selectedFrame = null
        resetSelectionTouch()
        mode?.finish()
        resetCursorBlink()
        invalidate()
    }

    private fun resetSelectionTouch() {
        draggedHandle = null
        selectionTouchOnHandle = false
        selectionTouchDecided = false
        selectionTouchX = 0f
        selectionTouchY = 0f
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
            if (selectedFrame != null || pinchInProgress) return false
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
            val current = session?.frame ?: return
            if (current.rows.isEmpty()) return
            selectedFrame = current
            val row = floor(event.y / cellHeight).toInt().coerceIn(0, current.rows.lastIndex)
            val range = wordSelection(current, row, event.x)
            anchor = range.first
            extent = range.second
            draggedHandle = SelectionHandle.END
            selectionTouchOnHandle = true
            selectionTouchDecided = true
            selectionTouchX = event.x
            selectionTouchY = event.y
            val mode = startActionMode(selectionActions, ActionMode.TYPE_FLOATING)
            if (mode == null) {
                clearSelection()
                return
            }
            actionMode = mode
            invalidate()
        }
    })
    private val scaleGestures = ScaleGestureDetector(context, object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
        override fun onScaleBegin(detector: ScaleGestureDetector): Boolean {
            if (!pinchZoomEnabled) return false
            pinchInProgress = true
            pinchFontSize = fontSize.toFloat()
            pinchChanged = false
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
            }
            return true
        }

        override fun onScaleEnd(detector: ScaleGestureDetector) {
            if (pinchChanged) onFontSizeChanged?.invoke(fontSize)
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
            if (selectedFrame != null) clearSelection()
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
        if (selectedFrame != null) {
            val frame = checkNotNull(selectedFrame)
            when (event.actionMasked) {
                MotionEvent.ACTION_POINTER_DOWN -> {
                    // A second finger exits selection first, then the regular
                    // pinch path below can take ownership of the stream.
                    clearSelection()
                    if (!pinchZoomEnabled) return true
                }
                MotionEvent.ACTION_DOWN -> {
                    val handle = handleAt(event.x, event.y)
                    if (handle != null) {
                        resetSelectionTouch()
                        draggedHandle = handle
                        val point = if (handle == SelectionHandle.START) anchor else extent
                        selectionTouchOnHandle = isNearVisualHandle(event.x, event.y, point)
                        selectionTouchX = event.x
                        selectionTouchY = event.y
                        return true
                    }
                    // A touch away from a handle dismisses selection and is
                    // replayed through the normal terminal gesture path.
                    clearSelection()
                }
                MotionEvent.ACTION_MOVE -> {
                    val handle = draggedHandle
                    if (handle != null && !selectionTouchDecided) {
                        val dx = abs(event.x - selectionTouchX)
                        val dy = abs(event.y - selectionTouchY)
                        val touchSlop = ViewConfiguration.get(context).scaledTouchSlop.toFloat()
                        if (max(dx, dy) < touchSlop) return true
                        if (!selectionTouchOnHandle && dy > dx) {
                            val downX = selectionTouchX
                            val downY = selectionTouchY
                            clearSelection()
                            replayPointerGesture(event, downX, downY)
                            return true
                        }
                        selectionTouchDecided = true
                    }
                    draggedHandle?.let { handle ->
                        val point = selectionPointAt(frame, event.x, event.y, handle)
                        if (handle == SelectionHandle.START) anchor = point else extent = point
                        actionMode?.invalidateContentRect()
                        invalidate()
                    }
                    return true
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    resetSelectionTouch()
                    return true
                }
                else -> return true
            }
        }
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

    private val selectionActions = object : ActionMode.Callback2() {
        override fun onCreateActionMode(mode: ActionMode, menu: Menu): Boolean {
            menu.add(0, android.R.id.copy, 0, android.R.string.copy)
            menu.add(0, android.R.id.paste, 1, android.R.string.paste)
            menu.add(0, android.R.id.selectAll, 2, android.R.string.selectAll)
            return true
        }
        override fun onPrepareActionMode(mode: ActionMode, menu: Menu) = false
        override fun onGetContentRect(mode: ActionMode, view: View, outRect: Rect) {
            selectedFrame?.let { outRect.set(selectionContentBounds()) }
                ?: outRect.set(0, 0, view.width, view.height)
        }
        override fun onActionItemClicked(mode: ActionMode, item: MenuItem): Boolean {
            when (item.itemId) {
                android.R.id.copy -> {
                    context.getSystemService(ClipboardManager::class.java).setPrimaryClip(ClipData.newPlainText("Pebrel", selectionText()))
                    mode.finish()
                }
                android.R.id.paste -> { pasteClipboard(); mode.finish() }
                android.R.id.selectAll -> {
                    selectedFrame?.takeIf { it.rows.isNotEmpty() }?.let {
                        anchor = SelectionPoint(0, 0)
                        extent = SelectionPoint(it.rows.lastIndex, it.columns)
                        actionMode?.invalidateContentRect()
                        invalidate()
                    }
                }
            }
            return true
        }
        override fun onDestroyActionMode(mode: ActionMode) {
            if (actionMode != null && actionMode !== mode) return
            selectedFrame = null
            resetSelectionTouch()
            actionMode = null
            resetCursorBlink()
            invalidate()
        }
    }
    private fun selectionText(): String {
        val frame = selectedFrame ?: return ""
        if (frame.rows.isEmpty()) return ""
        val low = if (anchor <= extent) anchor else extent
        val high = if (anchor <= extent) extent else anchor
        if (low == high) return ""
        val firstRow = low.row.coerceIn(0, frame.rows.lastIndex)
        val lastRow = high.row.coerceIn(firstRow, frame.rows.lastIndex)
        return (firstRow..lastRow).joinToString("\n") { y ->
            val from = if (y == low.row) low.column else 0
            val to = if (y == high.row) high.column else frame.columns
            selectedRowText(frame.rows[y], from, to, frame.columns)
        }.trimEnd()
    }
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
        return GhosttyInputConnection(this)
    }
    override fun onKeyDown(code: Int, event: KeyEvent): Boolean = handleKey(code, event, if (event.repeatCount > 0) 2 else 1) || super.onKeyDown(code, event)
    override fun onKeyUp(code: Int, event: KeyEvent): Boolean = handleKey(code, event, 0) || super.onKeyUp(code, event)
    private fun handleKey(code: Int, event: KeyEvent, action: Int): Boolean {
        if (!directInput || KeyEvent.isModifierKey(code) || code in setOf(KeyEvent.KEYCODE_BACK, KeyEvent.KEYCODE_VOLUME_UP, KeyEvent.KEYCODE_VOLUME_DOWN)) return false
        val mods = (if (event.isShiftPressed) 1 else 0) or (if (event.isCtrlPressed) 2 else 0) or (if (event.isAltPressed) 4 else 0) or (if (event.isMetaPressed) 8 else 0)
        val point = event.getUnicodeChar(event.metaState and KeyEvent.META_CTRL_MASK.inv())
        val text = if (point in 1..0x10ffff) String(Character.toChars(point)) else ""
        accept(session?.key(code, mods, action, text, event.getUnicodeChar(0)) == true)
        return true
    }
}
