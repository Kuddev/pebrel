package io.github.kuddev.pebrel.mobile.session

import android.app.Application
import android.content.Context
import android.graphics.Typeface
import android.view.MotionEvent
import androidx.test.core.app.ApplicationProvider
import io.github.kuddev.pebrel.terminal.TerminalSnapshotView
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.*

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], application = Application::class)
@ConscryptMode(ConscryptMode.Mode.OFF)
class DisplayPreferencesTest {
    private val context get() = ApplicationProvider.getApplicationContext<Context>()

    @Test fun nativePinchAndWrappingSurviveViewAndPreferencesRecreation() {
        context.getSharedPreferences("terminal_display", Context.MODE_PRIVATE).edit().clear().commit()
        val preferences = DisplayPreferences(context)
        val view = TerminalSnapshotView(context).apply {
            setFont(Typeface.MONOSPACE, 16)
            layout(0, 0, 600, 800)
            onZoomScaleChanged = { scale -> preferences.update { it.copy(desktopZoom = scale) } }
            // A tooltip recomposition must not overwrite an unfinished pinch with the stored scale.
            onZoomChanged = { _, _ -> setZoomScale(preferences.state.value.desktopZoom) }
        }
        var time = 10L
        fun touch(action: Int, vararg points: Pair<Float, Float>) {
            val properties = Array(points.size) { index -> MotionEvent.PointerProperties().apply { id = index; toolType = MotionEvent.TOOL_TYPE_FINGER } }
            val coordinates = Array(points.size) { index -> MotionEvent.PointerCoords().apply { x = points[index].first; y = points[index].second; pressure = 1f; size = 1f } }
            val event = MotionEvent.obtain(10L, time, action, points.size, properties, coordinates, 0, 0, 1f, 1f, 0, 0, android.view.InputDevice.SOURCE_TOUCHSCREEN, 0)
            view.onTouchEvent(event); event.recycle(); time += 20
        }
        touch(MotionEvent.ACTION_DOWN, 20f to 100f)
        touch(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), 20f to 100f, 120f to 100f)
        touch(MotionEvent.ACTION_MOVE, 5f to 100f, 180f to 100f)
        touch(MotionEvent.ACTION_MOVE, 0f to 100f, 230f to 100f)
        touch(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), 0f to 100f, 230f to 100f)
        touch(MotionEvent.ACTION_UP, 0f to 100f)
        assertTrue(view.zoomScale > 1f)
        preferences.update { it.copy(desktopWrapLines = true) }
        val restored = DisplayPreferences(context).state.value
        assertEquals(view.zoomScale, restored.desktopZoom, .001f)
        assertTrue(restored.desktopWrapLines)
        assertEquals(16, restored.fontSize)
        val reopened = TerminalSnapshotView(context).apply { setZoomScale(restored.desktopZoom); wrapLines = restored.desktopWrapLines }
        assertEquals(view.zoomScale, reopened.zoomScale, .001f)
        assertTrue(reopened.wrapLines)
    }

    @Test fun invalidPersistedZoomIsNormalizedWithoutChangingOtherPreferences() {
        assertEquals(1f, TerminalPreferences(desktopZoom = Float.NaN).normalized().desktopZoom)
        assertEquals(5f, TerminalPreferences(desktopZoom = 99f).normalized().desktopZoom)
        assertEquals(.5f, TerminalPreferences(desktopZoom = -3f).normalized().desktopZoom)
    }
}
