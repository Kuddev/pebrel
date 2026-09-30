package io.github.kuddev.pebrel.terminal

import android.content.ClipData
import android.content.ClipboardManager
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Rect
import android.os.Build
import android.view.ActionMode
import android.view.HapticFeedbackConstants
import android.view.Menu
import android.view.MenuItem
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.view.accessibility.AccessibilityNodeInfo
import android.widget.Toast
import kotlin.math.abs
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/** Shared physical-cell selection for live terminals and desktop mirrors. */
internal class TerminalSelection(
    private val view: View,
    private val canPaste: () -> Boolean,
    private val paste: (String) -> Boolean,
    private val changed: () -> Unit,
    private val startScroll: (MotionEvent, Float, Float) -> Unit,
) {
    private data class Point(val row: Int, val column: Int) : Comparable<Point> {
        override fun compareTo(other: Point) = if (row == other.row) column.compareTo(other.column) else row.compareTo(other.row)
    }
    private var anchor = Point(0, 0)
    private var extent = Point(0, 0)
    var frame: TerminalFrame? = null
        private set
    val active get() = frame != null
    private var mode: ActionMode? = null
    private var cellWidth = 1f
    private var cellHeight = 1f
    private var offsetX = 0f
    private var offsetY = 0f
    private val density get() = view.resources.displayMetrics.density
    private val radius get() = max(5f, 5f * density)
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private var dragging: Boolean? = null
    private var downX = 0f
    private var downY = 0f
    private var dragOffsetX = 0f
    private var dragOffsetY = 0f
    private var preciseTouch = false
    private var decided = false
    private var consumedKey: Int? = null

    fun geometry(width: Float, height: Float, x: Float = 0f, y: Float = 0f) {
        cellWidth = max(.1f, width)
        cellHeight = max(.1f, height)
        offsetX = x
        offsetY = y
    }

    fun begin(source: TerminalFrame, x: Float, y: Float): Boolean {
        if (source.rows.isEmpty() || source.columns == 0) return false
        clear()
        frame = source
        val rowIndex = floor((y + offsetY) / cellHeight).toInt().coerceIn(0, source.rows.lastIndex)
        val row = source.rows[rowIndex]
        var first = floor((x + offsetX) / cellWidth).toInt().coerceIn(0, source.columns - 1)
        var end = first + 1
        if (row != null) {
            first = leading(row, first)
            end = (first + span(row, first)).coerceAtMost(source.columns)
            val kind = wordClass(row, first)
            if (kind != 0) {
                while (first > 0) {
                    val previous = leading(row, first - 1)
                    if (wordClass(row, previous) != kind) break
                    first = previous
                }
                while (end < min(source.columns, row.cells.size / 6) && wordClass(row, end) == kind) end += span(row, end)
            }
        }
        anchor = Point(rowIndex, first)
        extent = Point(rowIndex, end)
        // 手指起点位于词中，手柄位于词尾；保留两者偏移，首个 MOVE 不应跳动选区。
        beginDrag(false, x, y, precise = true)
        mode = view.startActionMode(actions, ActionMode.TYPE_FLOATING)
        if (mode == null) { clear(); return false }
        view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
        changed()
        view.invalidate()
        return true
    }

    fun clear() {
        val previous = mode
        mode = null
        frame = null
        dragging = null
        anchor = Point(0, 0)
        extent = anchor
        previous?.finish()
        changed()
        view.invalidate()
    }

    private fun logicalX(point: Point) = point.column * cellWidth - offsetX
    private fun logicalY(point: Point) = (point.row + 1) * cellHeight - offsetY
    private fun clamp(value: Float, size: Int) = if (size <= radius * 2) size / 2f else value.coerceIn(radius, size - radius)
    private fun x(point: Point) = clamp(logicalX(point), view.width)
    private fun y(point: Point) = clamp(logicalY(point), view.height)

    private fun beginDrag(start: Boolean, touchX: Float, touchY: Float, precise: Boolean) {
        dragging = start
        downX = touchX
        downY = touchY
        val point = if (start) anchor else extent
        dragOffsetX = logicalX(point) - touchX
        dragOffsetY = logicalY(point) - touchY
        preciseTouch = precise
        decided = false
    }

    fun touch(event: MotionEvent): Boolean {
        val selected = frame ?: return false
        when (event.actionMasked) {
            MotionEvent.ACTION_POINTER_DOWN -> { clear(); return false }
            MotionEvent.ACTION_DOWN -> {
                fun distance(point: Point): Float {
                    val dx = (event.x - x(point)) / (24 * density)
                    val dy = (event.y - y(point)) / (18 * density)
                    return dx * dx + dy * dy
                }
                val start = distance(anchor) <= distance(extent)
                val point = if (start) anchor else extent
                if (distance(point) > 1f) { clear(); return false }
                val near = abs(event.x - x(point)) <= radius * 2 && abs(event.y - y(point)) <= radius * 2
                beginDrag(start, event.x, event.y, precise = near)
            }
            MotionEvent.ACTION_MOVE -> {
                val start = dragging ?: return true
                val dx = abs(event.x - downX)
                val dy = abs(event.y - downY)
                if (!decided) {
                    if (max(dx, dy) < ViewConfiguration.get(view.context).scaledTouchSlop) return true
                    if (!preciseTouch && dy > dx) {
                        val originalX = downX
                        val originalY = downY
                        clear()
                        startScroll(event, originalX, originalY)
                        return true
                    }
                    decided = true
                }
                val rowIndex = (ceil((event.y + dragOffsetY + offsetY) / cellHeight).toInt() - 1)
                    .coerceIn(0, selected.rows.lastIndex)
                val position = event.x + dragOffsetX + offsetX
                var column = (position / cellWidth).roundToInt().coerceIn(0, selected.columns)
                selected.rows[rowIndex]?.let { row ->
                    if (column in 1 until row.cells.size / 6 && width(row, column) == 0) {
                        val lead = leading(row, column)
                        column = if (position / cellWidth - lead < span(row, lead) / 2f) lead else lead + span(row, lead)
                    }
                }
                val point = Point(rowIndex, column)
                if (start) anchor = point else extent = point
                mode?.invalidateContentRect()
                view.invalidate()
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> { dragging = null; decided = false }
        }
        return true
    }

    fun draw(canvas: Canvas) {
        val selected = frame ?: return
        val low = minOf(anchor, extent)
        val high = maxOf(anchor, extent)
        val checkpoint = canvas.save()
        canvas.clipRect(0, 0, view.width, view.height)
        paint.color = selected.cursorColor
        paint.alpha = 70
        for (row in low.row..high.row) {
            val from = if (row == low.row) low.column else 0
            val to = if (row == high.row) high.column else selected.columns
            if (to > from) canvas.drawRect(from * cellWidth - offsetX, row * cellHeight - offsetY,
                to * cellWidth - offsetX, (row + 1) * cellHeight - offsetY, paint)
        }
        paint.alpha = 255
        canvas.drawCircle(x(anchor), y(anchor), radius, paint)
        canvas.drawCircle(x(extent), y(extent), radius, paint)
        canvas.restoreToCount(checkpoint)
    }

    private fun selectedText(): String {
        val selected = frame ?: return ""
        val low = minOf(anchor, extent)
        val high = maxOf(anchor, extent)
        return buildString {
            for (index in low.row..high.row) {
                val row = selected.rows[index]
                val from = if (index == low.row) low.column else 0
                val to = if (index == high.row) high.column else selected.columns
                val content = buildString {
                    if (row != null) {
                        var column = from
                        while (column < min(to, row.cells.size / 6)) {
                            if (width(row, column) == 0) { column++; continue }
                            append(text(row, column))
                            column += span(row, column)
                        }
                    }
                }
                // 手机列宽产生的软换行不属于原文，行尾空格也可能是命令的一部分。
                val wrapped = selected.wrapped?.getOrNull(index) == true
                append(if (to == selected.columns && !wrapped) content.trimEnd(' ') else content)
                if (index < high.row && !wrapped) append('\n')
            }
        }
    }

    private fun text(row: TerminalRow, column: Int): String {
        val index = column * 6
        if (index < 0 || index + 1 >= row.cells.size) return ""
        val start = row.cells[index].coerceIn(0, row.text.length)
        val end = (start + row.cells[index + 1].coerceAtLeast(0)).coerceIn(start, row.text.length)
        return row.text.substring(start, end)
    }
    private fun width(row: TerminalRow, column: Int) = row.cells.getOrNull(column * 6 + 2)?.coerceIn(0, 2) ?: 0
    private fun span(row: TerminalRow, column: Int) = width(row, column).coerceAtLeast(1)
    private fun leading(row: TerminalRow, column: Int): Int {
        var result = column.coerceIn(0, (row.cells.size / 6 - 1).coerceAtLeast(0))
        while (result > 0 && width(row, result) == 0) result--
        return result
    }
    private fun wordClass(row: TerminalRow, column: Int): Int {
        val cell = text(row, column)
        if (cell.isEmpty()) return 0
        val codePoint = cell.codePointAt(0)
        return when {
            Character.isLetterOrDigit(codePoint) || codePoint == '_'.code || Character.getType(codePoint) in
                setOf(Character.NON_SPACING_MARK.toInt(), Character.COMBINING_SPACING_MARK.toInt(), Character.ENCLOSING_MARK.toInt()) -> 1
            Character.isWhitespace(codePoint) -> 2
            else -> 0
        }
    }

    fun accessibility(info: AccessibilityNodeInfo) {
        if (!active) return
        info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_COPY)
        if (canPaste()) info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_PASTE)
    }

    fun accessibilityAction(action: Int): Boolean = when {
        !active -> false
        action == AccessibilityNodeInfo.ACTION_COPY -> this.action(android.R.id.copy)
        action == AccessibilityNodeInfo.ACTION_PASTE -> this.action(android.R.id.paste)
        else -> false
    }

    fun key(code: Int, event: android.view.KeyEvent, phase: Int): Boolean {
        if (phase == 0 && consumedKey == code) { consumedKey = null; return true }
        if (!active || phase == 0) return false
        val action = when {
            code == android.view.KeyEvent.KEYCODE_ESCAPE || code == android.view.KeyEvent.KEYCODE_BACK -> {
                consumedKey = code; clear(); return true
            }
            !event.isCtrlPressed -> return false
            code == android.view.KeyEvent.KEYCODE_C -> android.R.id.copy
            code == android.view.KeyEvent.KEYCODE_V -> android.R.id.paste
            code == android.view.KeyEvent.KEYCODE_A -> android.R.id.selectAll
            else -> return false
        }
        consumedKey = code
        return this.action(action)
    }

    fun action(id: Int): Boolean {
        if (!active) return false
        val context = view.context
        val clipboard = context.getSystemService(ClipboardManager::class.java)
        when (id) {
            android.R.id.copy -> {
                val content = selectedText()
                if (content.isEmpty()) return true
                try {
                    clipboard.setPrimaryClip(ClipData.newPlainText("Pebrel", content))
                    if (Build.VERSION.SDK_INT < 33) Toast.makeText(context, R.string.terminal_copied, Toast.LENGTH_SHORT).show()
                    view.announceForAccessibility(context.getString(R.string.terminal_copied))
                    clear()
                } catch (_: Exception) { Toast.makeText(context, R.string.terminal_copy_failed, Toast.LENGTH_SHORT).show() }
            }
            android.R.id.paste -> {
                if (!canPaste()) return true
                try {
                    val clip = clipboard.primaryClip ?: return true
                    if (clip.itemCount == 0) return true
                    if (paste(clip.getItemAt(0).coerceToText(context).toString())) clear()
                    else Toast.makeText(context, R.string.terminal_paste_rejected, Toast.LENGTH_SHORT).show()
                } catch (_: Exception) { Toast.makeText(context, R.string.terminal_paste_rejected, Toast.LENGTH_SHORT).show() }
            }
            android.R.id.selectAll -> frame?.let {
                anchor = Point(0, 0)
                extent = Point(it.rows.lastIndex, it.columns)
                mode?.invalidateContentRect()
                view.invalidate()
            }
            else -> return false
        }
        return true
    }

    private val actions = object : ActionMode.Callback2() {
        override fun onCreateActionMode(mode: ActionMode, menu: Menu): Boolean {
            menu.add(0, android.R.id.copy, 0, android.R.string.copy)
            menu.add(0, android.R.id.paste, 1, android.R.string.paste)
            menu.add(0, android.R.id.selectAll, 2, android.R.string.selectAll)
            return true
        }
        override fun onPrepareActionMode(mode: ActionMode, menu: Menu): Boolean {
            menu.findItem(android.R.id.paste)?.isVisible = canPaste()
            return true
        }
        override fun onGetContentRect(mode: ActionMode, view: View, outRect: Rect) {
            outRect.set(floor(min(x(anchor), x(extent)) - radius).toInt().coerceAtLeast(0),
                floor(min(y(anchor), y(extent)) - cellHeight).toInt().coerceAtLeast(0),
                ceil(max(x(anchor), x(extent)) + radius).toInt().coerceAtMost(view.width),
                ceil(max(y(anchor), y(extent)) + radius).toInt().coerceAtMost(view.height))
        }
        override fun onActionItemClicked(mode: ActionMode, item: MenuItem) = action(item.itemId)
        override fun onDestroyActionMode(mode: ActionMode) {
            if (this@TerminalSelection.mode === mode) clear()
        }
    }
}
