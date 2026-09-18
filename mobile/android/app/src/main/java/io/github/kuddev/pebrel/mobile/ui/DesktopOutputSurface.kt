package io.github.kuddev.pebrel.mobile.ui

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
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.TextButton
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.res.stringResource
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
) {
    if (frame != null || text.isEmpty()) {
        var copiedText by remember(identity) { mutableStateOf<String?>(null) }
        var lastKeyboardRequest by remember(identity) { mutableIntStateOf(keyboardRequest) }
        key(identity) {
            AndroidView(modifier = modifier, factory = { context -> TerminalSnapshotView(context) }, update = { view ->
                val app = view.context.applicationContext as PebrelApplication
                view.setFont(app.terminalTypeface(app.sessions.display.state.value.fontFamily), fontSize)
                view.pinchZoom = pinchZoom
                view.frame = frame
                view.contentDescription = frame?.text().orEmpty()
                view.onCopyRequested = { copiedText = it }
                view.inputTarget = inputTarget
                if (lastKeyboardRequest != keyboardRequest) {
                    lastKeyboardRequest = keyboardRequest
                    view.post { if (view.isAttachedToWindow) view.showKeyboard() }
                }
            })
        }
        copiedText?.let { content ->
            AlertDialog(onDismissRequest = { copiedText = null },
                title = { Text(stringResource(R.string.desktop_snapshot_text)) },
                text = { SelectionContainer { Text(content, maxLines = 12) } },
                confirmButton = { TextButton({ copiedText = null }) { Text(stringResource(R.string.close)) } })
        }
        return
    }
    // Keep the gesture handler's state object stable after a persisted zoom.
    // Replacing it while pointerInput retains the same identity makes the next
    // gesture write to a detached state object.
    var displayedSize by remember(identity) { mutableFloatStateOf(fontSize.toFloat()) }
    LaunchedEffect(identity, fontSize) { displayedSize = fontSize.toFloat() }
    val saveSize by rememberUpdatedState(onFontSize)
    val gestures = if (!pinchZoom) Modifier else Modifier.pointerInput(identity) {
        awaitEachGesture {
            awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
            var zooming = false
            do {
                val event = awaitPointerEvent(PointerEventPass.Initial)
                if (event.changes.count { it.pressed } >= 2) {
                    zooming = true
                    val zoom = event.calculateZoom()
                    if (zoom.isFinite() && zoom > 0f) {
                        displayedSize = (displayedSize * zoom).coerceIn(
                            TerminalPreferenceValues.MIN_FONT_SIZE.toFloat(),
                            TerminalPreferenceValues.MAX_FONT_SIZE.toFloat(),
                        )
                    }
                }
                // Finish the whole two-finger gesture before handing control
                // back, so the remaining finger does not cause a scroll jump.
                if (zooming) event.changes.forEach { it.consume() }
            } while (event.changes.any { it.pressed })
            if (zooming) saveSize(displayedSize.roundToInt())
        }
    }
    Column(modifier) {
        if (text.isNotBlank()) HelperText(stringResource(R.string.desktop_legacy_text), Modifier.padding(4.dp))
        Box(Modifier.weight(1f).fillMaxWidth().then(gestures)
            .verticalScroll(rememberScrollState()).horizontalScroll(rememberScrollState())) {
            SelectionContainer {
                Text(text, fontFamily = LocalTerminalFont.current, fontSize = displayedSize.sp,
                    softWrap = false, modifier = Modifier.padding(4.dp))
            }
        }
    }
}
