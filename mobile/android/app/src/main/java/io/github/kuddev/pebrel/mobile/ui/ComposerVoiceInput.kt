package io.github.kuddev.pebrel.mobile.ui

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.awaitLongPressOrCancellation
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.*
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupProperties
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.voice.*

@Composable
internal fun rememberComposerVoice(id: String, active: Boolean, onTranscript: (String) -> Unit): VoiceInputController {
    val context = LocalContext.current.applicationContext
    val scope = rememberCoroutineScope()
    val result by rememberUpdatedState(onTranscript)
    val controller = remember(id) { VoiceInputController(scope, VoiceModelStore(context)) { result(it) } }
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    DisposableEffect(controller, active, lifecycle) {
        if (active) controller.refresh() else controller.cancel()
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_STOP) controller.cancel()
        }
        lifecycle.addObserver(observer)
        onDispose { lifecycle.removeObserver(observer); controller.cancel() }
    }
    return controller
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ComposerVoiceButton(controller: VoiceInputController, state: VoiceState, enabled: Boolean) {
    val context = LocalContext.current
    val density = LocalDensity.current
    val haptic = LocalHapticFeedback.current
    val colors = MaterialTheme.colorScheme
    val cancelDistance = with(density) { 72.dp.toPx() }
    var modelSheet by remember(controller) { mutableStateOf(false) }
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        // A permission response never begins recording: the original finger may
        // already be up, or another session may now own the composer.
        if (!granted) controller.permissionDenied()
    }
    fun begin(): Boolean {
        if (!enabled) return false
        if (controller.state.value.phase != VoicePhase.Ready) { modelSheet = true; return false }
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            permission.launch(Manifest.permission.RECORD_AUDIO)
            return false
        }
        return controller.begin().also { if (it) haptic.performHapticFeedback(HapticFeedbackType.LongPress) }
    }
    val label = stringResource(R.string.voice_hold)
    val stopLabel = stringResource(R.string.voice_finish)
    val cancelLabel = stringResource(R.string.cancel)
    val startAction = { begin() }
    val latestBegin by rememberUpdatedState(startAction)
    Box {
        Box(Modifier.size(48.dp).clip(RoundedCornerShape(14.dp))
            .background(if (state.phase == VoicePhase.Recording) colors.primaryContainer else colors.surface.copy(alpha = 0f))
            .semantics {
                contentDescription = label
                role = Role.Button
                if (!enabled) disabled()
                onClick { if (enabled) modelSheet = true; enabled }
                customActions = if (state.phase == VoicePhase.Recording) listOf(
                    CustomAccessibilityAction(stopLabel) { controller.release(); true },
                    CustomAccessibilityAction(cancelLabel) { controller.cancel(); true },
                ) else listOf(CustomAccessibilityAction(label) { latestBegin() })
            }
            .pointerInput(controller, enabled) {
                if (!enabled) return@pointerInput
                awaitEachGesture {
                    val down = awaitFirstDown()
                    val held = awaitLongPressOrCancellation(down.id)
                    if (held == null) {
                        val end = currentEvent.changes.firstOrNull { it.id == down.id }
                        if (end != null && !end.pressed && !end.isConsumed &&
                            (end.position - down.position).getDistance() < viewConfiguration.touchSlop) modelSheet = true
                        return@awaitEachGesture
                    }
                    if (!latestBegin()) return@awaitEachGesture
                    var released = false
                    try {
                        do {
                            val event = awaitPointerEvent()
                            if (event.changes.count { it.pressed } > 1) break
                            val change = event.changes.firstOrNull { it.id == down.id } ?: break
                            if (change.isConsumed) break
                            controller.drag(change.position.y < down.position.y - cancelDistance)
                            change.consume()
                            if (!change.pressed) { released = true; controller.release(); break }
                        } while (true)
                    } finally { if (!released) controller.cancel() }
                }
            }, contentAlignment = Alignment.Center) {
            if (state.phase in setOf(VoicePhase.Checking, VoicePhase.Transcribing)) {
                CircularProgressIndicator(Modifier.size(22.dp), strokeWidth = 2.dp)
            } else Glyph(R.drawable.ic_microphone, Modifier.size(23.dp),
                if (enabled) colors.onSurfaceVariant else colors.onSurface.copy(alpha = .38f))
        }
        if (state.phase in setOf(VoicePhase.Recording, VoicePhase.Transcribing)) {
            Popup(alignment = Alignment.BottomCenter, offset = IntOffset(0, -with(density) { 58.dp.roundToPx() }),
                properties = PopupProperties(focusable = false, dismissOnClickOutside = false)) {
                Surface(shape = RoundedCornerShape(20.dp), tonalElevation = 6.dp,
                    color = if (state.cancelArmed) colors.errorContainer else colors.surfaceContainerHigh) {
                    Column(Modifier.width(248.dp).padding(20.dp), horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.spacedBy(10.dp)) {
                        Text(stringResource(when {
                            state.phase == VoicePhase.Transcribing -> R.string.voice_transcribing
                            state.cancelArmed -> R.string.voice_release_cancel
                            else -> R.string.voice_release_finish
                        }), style = MaterialTheme.typography.titleSmall)
                        if (state.phase == VoicePhase.Recording) {
                            VoiceMeter(state.level, if (state.cancelArmed) colors.error else colors.primary)
                            Text(stringResource(R.string.voice_elapsed, state.seconds), fontSize = 12.sp)
                            Text(stringResource(R.string.voice_slide_cancel), fontSize = 12.sp)
                        }
                        TextButton({ controller.cancel() }) { Text(cancelLabel) }
                    }
                }
            }
        }
    }
    if (modelSheet) {
        ModalBottomSheet(onDismissRequest = { modelSheet = false }) {
            Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp).padding(bottom = 32.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text("Whisper · Base", style = MaterialTheme.typography.titleLarge)
                Text(stringResource(R.string.voice_model_description))
                when (state.phase) {
                    VoicePhase.Downloading -> {
                        LinearProgressIndicator(progress = { state.progress }, modifier = Modifier.fillMaxWidth())
                        Text(stringResource(R.string.voice_download_progress, (state.progress * 100).toInt()))
                        TextButton({ controller.cancel() }) { Text(cancelLabel) }
                    }
                    VoicePhase.Missing -> Button({ controller.download() }) { Text(stringResource(R.string.voice_download)) }
                    VoicePhase.Ready -> {
                        Text(stringResource(R.string.voice_model_ready))
                        TextButton({ controller.remove() }) { Text(stringResource(R.string.voice_remove)) }
                    }
                    else -> CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
                }
                if (state.error != null) VoiceErrorText(state.error)
            }
        }
    }
}

@Composable
internal fun VoiceErrorText(error: VoiceError) {
    val message = when (error) {
        VoiceError.Permission -> R.string.voice_permission
        VoiceError.Failed -> R.string.composer_voice_failed
        VoiceError.Empty -> R.string.composer_voice_empty
        VoiceError.Short -> R.string.voice_too_short
        VoiceError.Model -> R.string.voice_model_failed
        VoiceError.Engine -> R.string.voice_engine_failed
    }
    Text(stringResource(message), Modifier.padding(horizontal = 12.dp), color = MaterialTheme.colorScheme.error,
        fontSize = 12.sp, lineHeight = 18.sp)
}

@Composable
private fun VoiceMeter(level: Float, color: androidx.compose.ui.graphics.Color) {
    Canvas(Modifier.width(140.dp).height(36.dp)) {
        val gap = size.width / 11
        for (i in 0 until 11) {
            val shape = 1f - kotlin.math.abs(i - 5) / 7f
            val height = (4.dp.toPx() + size.height * (level * 8).coerceIn(0f, 1f) * shape).coerceAtMost(size.height)
            drawRoundRect(color, Offset(i * gap + gap / 4, (size.height - height) / 2),
                Size(gap / 2, height), androidx.compose.ui.geometry.CornerRadius(gap / 4))
        }
    }
}
