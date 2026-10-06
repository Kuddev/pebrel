package io.github.kuddev.pebrel.mobile.ui

import android.view.View
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Alignment
import kotlinx.coroutines.launch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.unit.em
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.viewinterop.AndroidView
import io.github.kuddev.pebrel.terminal.TerminalFrame
import io.github.kuddev.pebrel.terminal.TerminalSnapshotView
import io.github.kuddev.pebrel.terminal.TerminalInputTarget
import io.github.kuddev.pebrel.mobile.PebrelApplication
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.session.TerminalPreferenceValues
import kotlin.math.roundToInt

/** Only two-finger gestures are claimed; one-finger scroll/selection is unchanged. */
@Composable
internal fun DesktopOutputSurface(
    identity: String,
    text: String,
    fontSize: Int,
    pinchZoom: Boolean,
    onFontSize: (Int) -> Unit,
    modifier: Modifier = Modifier,
    frame: TerminalFrame? = null,
    inputTarget: TerminalInputTarget? = null,
    keyboardRequest: Int = 0,
    loading: Boolean = false,
    connected: Boolean = true,
    wrapLines: Boolean = true,
    pasteTarget: TerminalInputTarget? = inputTarget,
    scrollTarget: TerminalInputTarget? = inputTarget,
) {
    val legacyText = frame == null && text.isNotEmpty()
    var lastKeyboardRequest by remember(identity) { mutableIntStateOf(keyboardRequest) }
    var inputView by remember(identity) { mutableStateOf<TerminalSnapshotView?>(null) }
    val zoomFeedback = rememberTerminalZoomFeedback(identity)
    Box(modifier) {
        // 旧桌面只返回文本，也必须保留同一原生 IME 所有者；不把输出伪装成可编辑文本或虚构光标。
        key(identity) {
            AndroidView(modifier = Modifier.fillMaxSize(), factory = { context -> TerminalSnapshotView(context) },
                onRelease = { view ->
                    view.inputTarget = null
                    view.pasteTarget = null
                    view.scrollTarget = null
                    view.onZoomChanged = null
                    if (inputView === view) inputView = null
                }, update = { view ->
                inputView = view
                val app = view.context.applicationContext as PebrelApplication
                view.setFont(app.terminalTypeface(app.sessions.display.state.value.fontFamily), fontSize)
                view.pinchZoom = pinchZoom
                view.wrapLines = wrapLines
                view.onZoomChanged = zoomFeedback::update
                view.frame = frame
                view.contentDescription = frame?.text().orEmpty()
                view.importantForAccessibility = if (legacyText) View.IMPORTANT_FOR_ACCESSIBILITY_NO
                    else View.IMPORTANT_FOR_ACCESSIBILITY_AUTO
                view.inputTarget = inputTarget
                view.pasteTarget = pasteTarget
                view.scrollTarget = scrollTarget
                if (lastKeyboardRequest != keyboardRequest) {
                    lastKeyboardRequest = keyboardRequest
                    view.post { if (view.isAttachedToWindow) view.showKeyboard() }
                }
            })
        }
        if (legacyText) {
            LegacyDesktopText(identity, text, fontSize, pinchZoom, onFontSize,
                onKeyboard = if (inputTarget == null) null else ({ inputView?.showKeyboard() }),
                onZoomChanged = zoomFeedback::update, wrapLines = wrapLines)
        } else if (frame == null && connected) {
            HelperText(stringResource(if (loading) R.string.terminal_loading else R.string.terminal_waiting),
                Modifier.align(Alignment.Center).padding(16.dp))
        }
        TerminalZoomIndicator(zoomFeedback)
    }
}

@Composable
private fun LegacyDesktopText(
    identity: String,
    text: String,
    fontSize: Int,
    pinchZoom: Boolean,
    onFontSize: (Int) -> Unit,
    onKeyboard: (() -> Unit)?,
    onZoomChanged: (Int, Boolean) -> Unit,
    wrapLines: Boolean,
) {
    // Keep the gesture handler's state object stable after a persisted zoom.
    // Replacing it while pointerInput retains the same identity makes the next
    // gesture write to a detached state object.
    var displayedSize by remember(identity) { mutableFloatStateOf(fontSize.toFloat()) }
    LaunchedEffect(identity, fontSize) { displayedSize = fontSize.toFloat() }
    val saveSize by rememberUpdatedState(onFontSize)
    val showKeyboard by rememberUpdatedState(onKeyboard)
    val reportZoom by rememberUpdatedState(onZoomChanged)
    val scroll = rememberScrollState()
    val scope = rememberCoroutineScope()
    var previousExtent by remember(identity) { mutableIntStateOf(0) }
    LaunchedEffect(identity, text, scroll.maxValue) {
        // 首帧显示最新输出；用户上翻后保留阅读位置，不裁掉原始空行或伪造光标。
        val follow = scroll.value >= minOf(previousExtent, scroll.maxValue) - 1
        previousExtent = scroll.maxValue
        if (follow && !scroll.isScrollInProgress) scroll.scrollTo(scroll.maxValue)
    }
    val keyboardLabel = stringResource(R.string.composer_show_keyboard)
    val inputGestures = if (onKeyboard == null) Modifier else Modifier
        .semantics { onClick(keyboardLabel) { showKeyboard?.invoke(); true } }
        .pointerInput(identity) {
            awaitEachGesture {
                val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Final)
                var tapped = !down.isConsumed
                do {
                    val event = awaitPointerEvent(PointerEventPass.Final)
                    // 只观察未被选区、滚动或缩放认领的短点按，不消费事件，保留原有阅读手势。
                    if (event.changes.any { it.isConsumed || it.id != down.id ||
                            (it.position - down.position).getDistance() > viewConfiguration.touchSlop ||
                            it.uptimeMillis - down.uptimeMillis >= viewConfiguration.longPressTimeoutMillis }) {
                        tapped = false
                    }
                } while (event.changes.any { it.pressed })
                if (tapped) showKeyboard?.invoke()
            }
        }
    val gestures = if (!pinchZoom) Modifier else Modifier.pointerInput(identity) {
        awaitEachGesture {
            awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
            var zooming = false
            do {
                val event = awaitPointerEvent(PointerEventPass.Initial)
                if (event.changes.count { it.pressed } >= 2) {
                    if (!zooming) reportZoom(displayedSize.roundToInt(), true)
                    zooming = true
                    val zoom = event.calculateZoom()
                    if (zoom.isFinite() && zoom > 0f) {
                        displayedSize = (displayedSize * zoom).coerceIn(
                            TerminalPreferenceValues.MIN_FONT_SIZE.toFloat(),
                            TerminalPreferenceValues.MAX_FONT_SIZE.toFloat(),
                        )
                        reportZoom(displayedSize.roundToInt(), true)
                    }
                }
                // Finish the whole two-finger gesture before handing control
                // back, so the remaining finger does not cause a scroll jump.
                if (zooming) event.changes.forEach { it.consume() }
            } while (event.changes.any { it.pressed })
            if (zooming) {
                val size = displayedSize.roundToInt()
                saveSize(size)
                reportZoom(size, false)
            }
        }
    }
    Box(Modifier.fillMaxSize()) {
        Box(Modifier.fillMaxSize().then(gestures).then(inputGestures)
            .verticalScroll(scroll).then(if (wrapLines) Modifier else Modifier.horizontalScroll(rememberScrollState()))) {
            SelectionContainer {
                Text(text, fontFamily = LocalTerminalFont.current, fontSize = displayedSize.sp,
                    // 固定正文行高会只放大横向字宽；终端行距必须随字号等比变化。
                    lineHeight = 1.25.em, letterSpacing = 0.sp,
                    softWrap = wrapLines, modifier = Modifier.padding(4.dp).then(if (wrapLines) Modifier.fillMaxWidth() else Modifier))
            }
        }
        if (scroll.canScrollForward) TextButton(
            onClick = { scope.launch { scroll.scrollTo(scroll.maxValue) } },
            modifier = Modifier.align(Alignment.BottomEnd).padding(8.dp),
        ) { Text(stringResource(R.string.terminal_latest_output)) }
    }
}
