package io.github.kuddev.pebrel.mobile.ui

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Typeface
import android.view.InputDevice
import android.view.MotionEvent
import androidx.test.core.app.ApplicationProvider
import io.github.kuddev.pebrel.mobile.connection.decodeDesktopScreen
import io.github.kuddev.pebrel.terminal.GhosttyView
import io.github.kuddev.pebrel.terminal.SessionTransport
import io.github.kuddev.pebrel.terminal.TerminalCallbacks
import io.github.kuddev.pebrel.terminal.TerminalFrame
import io.github.kuddev.pebrel.terminal.TerminalRow
import io.github.kuddev.pebrel.terminal.TerminalSession
import io.github.kuddev.pebrel.terminal.TerminalSnapshotView
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.File

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class TerminalSnapshotViewTest {
    private val red = 0xffff0000.toInt()
    private val green = 0xff00ff00.toInt()
    private val background = 0xff101010.toInt()

    private fun row(text: String, foreground: Int = red, fill: Int = background, flags: Int = 0): TerminalRow {
        val cells = IntArray(text.length * 6)
        text.indices.forEach { x ->
            intArrayOf(x, 1, 1, foreground, fill, flags).copyInto(cells, x * 6)
        }
        return TerminalRow(text, cells)
    }

    private fun view(vararg rows: TerminalRow, width: Int = 240, height: Int = 120): TerminalSnapshotView {
        val context = ApplicationProvider.getApplicationContext<Context>()
        return TerminalSnapshotView(context).apply {
            setFont(Typeface.MONOSPACE, 20)
            frame = TerminalFrame(arrayOf(*rows), intArrayOf(rows[0].cells.size / 6, rows.size, 0, 0, 0,
                this@TerminalSnapshotViewTest.background, red, 2))
            layout(0, 0, width, height)
        }
    }

    private fun render(view: TerminalSnapshotView): Bitmap =
        Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888).also { view.draw(Canvas(it)) }

    private fun colorBounds(bitmap: Bitmap, color: Int): android.graphics.Rect {
        val bounds = android.graphics.Rect(bitmap.width, bitmap.height, 0, 0)
        for (y in 0 until bitmap.height) for (x in 0 until bitmap.width) {
            if (bitmap.getPixel(x, y) == color) {
                bounds.left = minOf(bounds.left, x)
                bounds.top = minOf(bounds.top, y)
                bounds.right = maxOf(bounds.right, x + 1)
                bounds.bottom = maxOf(bounds.bottom, y + 1)
            }
        }
        return bounds
    }

    @Test fun narrowAndWideViewsPreserveColoredBlocksOnOnePhysicalRow() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val view = TerminalSnapshotView(context)
        val row = TerminalRow("██", intArrayOf(0, 1, 1, red, background, 0, 1, 1, 1, green, background, 0))
        view.frame = TerminalFrame(arrayOf(row), intArrayOf(2, 1, 0, 0, 0, background, red, 2))
        for (width in listOf(12, 120)) {
            view.layout(0, 0, width, 80)
            val image = Bitmap.createBitmap(width, 80, Bitmap.Config.ARGB_8888)
            view.draw(Canvas(image))
            val redRows = mutableSetOf<Int>()
            val greenRows = mutableSetOf<Int>()
            for (y in 0 until image.height) for (x in 0 until image.width) {
                when (image.getPixel(x, y)) { red -> redRows += y; green -> greenRows += y }
            }
            assertTrue("red foreground must reach pixels", redRows.isNotEmpty())
            assertEquals("next colored cell must not wrap to another row", redRows, greenRows)
            assertTrue(redRows.size < image.height / 2)
            image.recycle()
        }
    }

    @Test fun backgroundCannotEraseSiblingChromeOutsideTheView() {
        val view = view(row("██"), width = 80, height = 60)
        val bitmap = Bitmap.createBitmap(140, 120, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        canvas.drawColor(Color.MAGENTA)
        canvas.translate(20f, 30f)
        view.draw(canvas)
        assertEquals("left sibling", Color.MAGENTA, bitmap.getPixel(19, 50))
        assertEquals("top chrome", Color.MAGENTA, bitmap.getPixel(40, 29))
        assertEquals("right sibling", Color.MAGENTA, bitmap.getPixel(100, 50))
        assertEquals("bottom composer", Color.MAGENTA, bitmap.getPixel(40, 90))
        assertEquals(background, bitmap.getPixel(90, 80))
        bitmap.recycle()
    }

    @Test fun mascotQuadrantsAndBlankBackgroundsHaveNoFontGaps() {
        val view = view(row("▐▛███▜▌"), row("       ", fill = green))
        val bitmap = render(view)
        val block = colorBounds(bitmap, red)
        val fill = colorBounds(bitmap, green)
        assertEquals("adjacent physical rows share an edge", block.bottom, fill.top)
        val cw = fill.width() / 7f
        val ch = fill.height().toFloat()
        fun sample(cell: Int, x: Float, y: Float) = bitmap.getPixel(((cell + x) * cw).toInt(), (y * ch).toInt())
        assertEquals(background, sample(0, .25f, .25f))
        assertEquals(red, sample(0, .75f, .25f))
        assertEquals(background, sample(1, .75f, .75f))
        assertEquals(red, sample(1, .25f, .75f))
        assertEquals(background, sample(5, .25f, .75f))
        assertEquals(red, sample(5, .75f, .75f))
        assertEquals(background, sample(6, .75f, .25f))
        for (x in (2 * cw).toInt() until (5 * cw).toInt()) {
            assertEquals("full block seam at $x", red, bitmap.getPixel(x, (ch / 2).toInt()))
        }
        bitmap.recycle()
    }

    @Test fun hiddenGlyphKeepsItsBackgroundAndDimDoesNotAffectTheNextCell() {
        val cells = intArrayOf(0, 1, 1, red, green, 32, 1, 1, 1, red, background, 16,
            2, 1, 1, red, background, 0)
        val view = view(TerminalRow("███", cells))
        val bitmap = render(view)
        val hidden = colorBounds(bitmap, green)
        val cw = hidden.width()
        val y = hidden.height() / 2
        assertEquals(green, bitmap.getPixel(cw / 2, y))
        val dim = bitmap.getPixel(cw + cw / 2, y)
        assertTrue(Color.red(dim) in 100..220)
        assertEquals(red, bitmap.getPixel(cw * 2 + cw / 2, y))
        bitmap.recycle()
    }

    @Test fun liveLocalAndSshPainterMatchesPcColorsAndMixedWidthPlacement() {
        val theme = intArrayOf(Color.WHITE, background, Color.WHITE) + IntArray(16) { Color.RED }
        val frame = decodeDesktopScreen(JSONObject("""{"version":1,"columns":8,"rows":[
            [["中",2,14251863,-258,0],["é",1,14251863,-258,0],["😀",2,14251863,-258,0],
             ["█",1,65280,-258,0],["A",1,14251863,-258,0],[" ",1,-257,255,0]]
            ],"cursor":[0,0,0],"palette":[]}"""), theme)
        val context = ApplicationProvider.getApplicationContext<Context>()
        val mirror = view(frame.rows[0]!!).apply { this.frame = frame }
        // Exercise the production live View with a prepared JNI-shaped frame;
        // no native parser/SSH connection is claimed by this Canvas regression.
        val transport = object : SessionTransport {
            override fun open(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) = Unit
            override fun input() = ByteArrayInputStream(byteArrayOf())
            override fun output() = ByteArrayOutputStream()
            override fun resize(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) = Unit
            override fun awaitExit() = 0
            override fun close() = Unit
        }
        val session = TerminalSession(transport, TerminalCallbacks())
        TerminalSession::class.java.getDeclaredField("frame").apply { isAccessible = true }.set(session, frame)
        val live = GhosttyView(context).apply {
            setFont(Typeface.MONOSPACE, 20 * resources.displayMetrics.scaledDensity)
            this.session = session
            layout(0, 0, mirror.width, mirror.height)
        }
        try {
            val pcImage = render(mirror)
            val liveImage = Bitmap.createBitmap(live.width, live.height, Bitmap.Config.ARGB_8888)
            live.draw(Canvas(liveImage))
            assertTrue("same frame must have identical local/SSH/PC pixels", pcImage.sameAs(liveImage))
            val marker = colorBounds(pcImage, green)
            assertFalse(marker.isEmpty)
            val lastBackground = colorBounds(pcImage, Color.BLUE)
            assertFalse(lastBackground.isEmpty)
            val cw = lastBackground.width()
            assertEquals("CJK + combining + emoji occupy 5 cells, not their UTF-16 length", 5f,
                marker.left.toFloat() / cw, .15f)
            val report = File("build/reports/terminal-color/mixed-width.png")
            val reportDirectory = checkNotNull(report.parentFile)
            check(reportDirectory.mkdirs() || reportDirectory.isDirectory)
            report.outputStream().use { check(pcImage.compress(Bitmap.CompressFormat.PNG, 100, it)) }
            pcImage.recycle()
            liveImage.recycle()
        } finally {
            live.session = null
            session.finishIfRunning()
        }
    }

    @Test fun keyboardHeightChangeKeepsTheLatestPhysicalRowVisible() {
        val rows = Array(20) { row("████", if (it == 19) green else red) }
        val view = view(*rows, height = 200)
        assertFalse(colorBounds(render(view), green).isEmpty)
        view.layout(0, 0, 240, 70)
        val image = render(view)
        assertFalse("opening keyboard must not lose the tail", colorBounds(image, green).isEmpty)
        image.recycle()
    }

    private var eventTime = 1_000L
    private fun touch(view: TerminalSnapshotView, down: Long, action: Int, vararg points: Pair<Float, Float>) {
        eventTime += 20
        val properties = Array(points.size) { MotionEvent.PointerProperties().apply { id = it; toolType = MotionEvent.TOOL_TYPE_FINGER } }
        val coordinates = Array(points.size) { i -> MotionEvent.PointerCoords().apply {
            x = points[i].first; y = points[i].second; pressure = 1f; size = 1f
        } }
        val event = MotionEvent.obtain(down, eventTime, action, points.size, properties, coordinates,
            0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_TOUCHSCREEN, 0)
        view.dispatchTouchEvent(event)
        event.recycle()
    }

    private fun pinch(view: TerminalSnapshotView) {
        val down = eventTime
        touch(view, down, MotionEvent.ACTION_DOWN, 20f to 40f)
        touch(view, down, MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), 20f to 40f, 120f to 40f)
        touch(view, down, MotionEvent.ACTION_MOVE, 5f to 40f, 180f to 40f)
        touch(view, down, MotionEvent.ACTION_MOVE, 0f to 40f, 230f to 40f)
        touch(view, down, MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), 0f to 40f, 230f to 40f)
        touch(view, down, MotionEvent.ACTION_UP, 0f to 40f)
    }

    @Test fun successivePinchesZoomThePixelsWithoutMutatingTheGrid() {
        val view = view(row("████"), height = 200)
        val original = view.frame
        val before = colorBounds(render(view), red).height()
        pinch(view)
        val first = colorBounds(render(view), red).height()
        pinch(view)
        val second = colorBounds(render(view), red).height()
        assertTrue("first pinch", first > before)
        assertTrue("second pinch", second > first)
        assertSame("phone zoom must not resize the PC's grid", original, view.frame)
        view.pinchZoom = false
        pinch(view)
        assertEquals(second, colorBounds(render(view), red).height())
    }
}
