package io.github.kuddev.pebrel.mobile.ui

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Typeface
import androidx.test.core.app.ApplicationProvider
import io.github.kuddev.pebrel.terminal.TerminalFrame
import io.github.kuddev.pebrel.terminal.TerminalRow
import io.github.kuddev.pebrel.terminal.TerminalSnapshotView
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.math.floor

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class CharacterGridTest {
    private val context get() = ApplicationProvider.getApplicationContext<Context>()
    private val font get() = Typeface.createFromAsset(context.assets, "terminal.ttf")

    private fun row(text: String) = TerminalRow(text, IntArray(text.length * 6).also { cells ->
        for (x in text.indices) {
            val offset = x * 6
            cells[offset] = x; cells[offset + 1] = 1; cells[offset + 2] = 1
            cells[offset + 3] = Color.WHITE; cells[offset + 4] = Color.BLACK
        }
    })

    private fun view(rows: Array<TerminalRow>, pixels: Int) = TerminalSnapshotView(context).apply {
        setFont(font, pixels)
        layout(0, 0, 1000, 1000)
        frame = TerminalFrame(arrayOf(*rows), intArrayOf(rows.first().cells.size / 6, rows.size, 0, 0, 0, Color.BLACK, Color.WHITE, 2))
    }

    private fun metric(view: TerminalSnapshotView, name: String) = TerminalSnapshotView::class.java
        .getDeclaredField(name).apply { isAccessible = true }.getFloat(view)

    private fun render(view: TerminalSnapshotView) = Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888).also {
        view.draw(Canvas(it))
    }

    @Test fun everyBraillePatternUsesTheSameTwoByFourGridAtThreeScales() {
        val positions = arrayOf(0 to 0, 0 to 1, 0 to 2, 1 to 0, 1 to 1, 1 to 2, 0 to 3, 1 to 3)
        for (size in listOf(14, 21, 28)) {
            val rows = Array(16) { y -> row(String(CharArray(16) { x -> (0x2800 + y * 16 + x).toChar() })) }
            val view = view(rows, size)
            val cw = metric(view, "cellWidth")
            val ch = metric(view, "cellHeight")
            assertEquals(floor(size * context.resources.displayMetrics.scaledDensity * 1.2f), ch, .001f)
            val bitmap = render(view)
            for (mask in 0..255) for ((bit, position) in positions.withIndex()) {
                val x = ((mask % 16 + (position.first + .5f) / 2f) * cw).toInt()
                val y = ((mask / 16 + (position.second + .5f) / 4f) * ch).toInt()
                val lit = Color.red(bitmap.getPixel(x, y)) > 128
                assertEquals("size=$size mask=$mask bit=$bit", mask and (1 shl bit) != 0, lit)
            }
            assertEquals("source text is not replaced by drawn shapes", rows[0].text, view.frame!!.rows[0]!!.text)
            bitmap.recycle()
        }
    }

    @Test fun accentsAndDescendersAreNotClippedOrPaintedOverByAdjacentRowBackgrounds() {
        val text = "Åg中éj"
        for (size in listOf(14, 21, 28)) {
            val view = view(arrayOf(row("     "), row(text), row("     ")), size)
            val bitmap = render(view)
            val cw = metric(view, "cellWidth")
            val ch = metric(view, "cellHeight")
            val baseline = metric(view, "baseline")
            val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
                typeface = font; textSize = size * context.resources.displayMetrics.scaledDensity; color = Color.WHITE
                fontFeatureSettings = "'liga' 0, 'calt' 0"
            }
            val reference = Bitmap.createBitmap(bitmap.width, bitmap.height, Bitmap.Config.ARGB_8888)
            val canvas = Canvas(reference)
            canvas.drawColor(Color.BLACK)
            // Compare complete visible ink with the same font drawn without row clipping.
            for (i in text.indices) {
                val save = canvas.save()
                canvas.clipRect(i * cw, 0f, (i + 1) * cw, reference.height.toFloat())
                val advance = paint.measureText(text, i, i + 1)
                if (advance > cw) canvas.scale(cw / advance, 1f, i * cw, ch)
                canvas.drawTextRun(text, i, i + 1, i, i + 1, i * cw, ch + baseline, false, paint)
                canvas.restoreToCount(save)
            }
            assertTrue("complete CJK/accent/descender ink at size=$size", reference.sameAs(bitmap))
            reference.recycle(); bitmap.recycle()
        }
    }
}
