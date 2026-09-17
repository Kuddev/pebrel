package io.github.kuddev.pebrel.mobile.ui

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.speech.RecognizerIntent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.session.SessionRepository
import kotlinx.coroutines.launch

private enum class VoiceFeedback {
    None,
    Unavailable,
    Failed,
    Empty,
    TooLong,
}

@Composable
fun CommandComposer(
    id: String,
    repository: SessionRepository,
    enabled: Boolean,
    direct: Boolean,
    onDirect: ((Boolean) -> Unit)?,
    onKey: ((String) -> Unit)?,
    onKeyboard: (() -> Unit)? = null,
    focused: Boolean = false,
    onToggleFocus: (() -> Unit)? = null,
    onAttach: (() -> Unit)? = null,
    attachmentBusy: Boolean = false,
    send: suspend (String) -> Boolean,
) {
    val drafts by repository.drafts.collectAsStateWithLifecycle()
    val preferences by repository.display.state.collectAsStateWithLifecycle()
    val historyBySession by repository.commandHistory.collectAsStateWithLifecycle()
    val draft = drafts[id].orEmpty()
    val history = historyBySession[id].orEmpty()
    val scope = rememberCoroutineScope()
    val focusRequester = remember { FocusRequester() }
    val keyboard = LocalSoftwareKeyboardController.current
    val density = LocalDensity.current
    val keyboardVisible = WindowInsets.ime.getBottom(density) > 0
    val topInset = WindowInsets.statusBars.getTop(density)
    val motion = rememberPebrelMotion()
    val colors = MaterialTheme.colorScheme
    val editorInteractions = remember(id) { MutableInteractionSource() }
    val editorFocused by editorInteractions.collectIsFocusedAsState()
    val voicePrompt = stringResource(R.string.composer_voice_prompt)
    var sending by remember(id) { mutableStateOf(false) }
    var keys by rememberSaveable(id) { mutableStateOf(false) }
    var editorExpanded by rememberSaveable(id) { mutableStateOf(false) }
    var selectedHistory by rememberSaveable(id) { mutableStateOf<String?>(null) }
    var voiceFeedback by remember(id) { mutableStateOf(VoiceFeedback.None) }
    var historyDismissed by rememberSaveable(id) { mutableStateOf(false) }
    var voiceOwner by remember { mutableStateOf<String?>(null) }
    var editorWidth by remember { mutableIntStateOf(0) }
    var historyRoom by remember { mutableStateOf(0.dp) }

    val voiceLauncher = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        if (voiceOwner == id) {
            voiceOwner = null
            when (result.resultCode) {
                Activity.RESULT_OK -> {
                    val spoken = result.data?.getStringArrayListExtra(RecognizerIntent.EXTRA_RESULTS)
                        ?.firstOrNull()
                        ?.trim()
                    if (spoken.isNullOrEmpty()) {
                        voiceFeedback = VoiceFeedback.Empty
                    } else {
                        val latestDraft = repository.drafts.value[id].orEmpty()
                        val next = if (latestDraft.isBlank()) spoken else
                            latestDraft + if (latestDraft.last().isWhitespace()) spoken else " $spoken"
                        if (next.length > 8192) {
                            voiceFeedback = VoiceFeedback.TooLong
                        } else {
                            selectedHistory = null
                            repository.setDraft(id, next)
                            voiceFeedback = VoiceFeedback.None
                        }
                    }
                }
                Activity.RESULT_CANCELED -> {
                    // Cancelling the recognizer leaves the existing draft untouched.
                    voiceFeedback = VoiceFeedback.None
                }
                else -> voiceFeedback = VoiceFeedback.Failed
            }
        }
    }

    fun updateDraft(value: String) {
        selectedHistory = null
        historyDismissed = false
        repository.setDraft(id, value)
    }

    fun launchVoice() {
        voiceFeedback = VoiceFeedback.None
        voiceOwner = id
        val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
            putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
            putExtra(RecognizerIntent.EXTRA_PROMPT, voicePrompt)
            putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, false)
        }
        try {
            voiceLauncher.launch(intent)
        } catch (_: ActivityNotFoundException) {
            voiceOwner = null
            voiceFeedback = VoiceFeedback.Unavailable
        } catch (_: SecurityException) {
            voiceOwner = null
            voiceFeedback = VoiceFeedback.Unavailable
        }
    }

    fun sendDraft(command: String) {
        sending = true
        scope.launch {
            try {
                if (send(command)) repository.acknowledgeDraft(id, command)
                else if (repository.error.value == null) repository.error.value = "input_rejected"
            } finally {
                sending = false
            }
        }
    }

    fun requestSend() {
        if (enabled && !sending && draft.isNotBlank()) sendDraft(draft)
    }

    LaunchedEffect(id) {
        voiceOwner = null
    }
    LaunchedEffect(draft, selectedHistory) {
        if (selectedHistory != null && selectedHistory != draft) selectedHistory = null
    }
    val historyItems = remember(history, draft, preferences.suggestions, direct) {
        if (direct || !preferences.suggestions) emptyList() else {
            val query = draft.trim()
            history.asSequence()
                .filter(String::isNotEmpty)
                .filter { query.isEmpty() || it.contains(query, ignoreCase = true) }
                .distinct()
                .toList()
        }
    }
    val showHistory = historyItems.isNotEmpty() && draft.isNotBlank() && editorFocused && !historyDismissed && historyRoom >= 52.dp
    val shortcuts = listOf("Ctrl+C", "Esc", "Tab", "←", "→", "↑", "↓")
    val editorEnter = if (motion.animationsEnabled) expandVertically() + fadeIn() else EnterTransition.None
    val editorExit = if (motion.animationsEnabled) shrinkVertically() + fadeOut() else ExitTransition.None

    Column(Modifier.fillMaxWidth().padding(horizontal = 6.dp, vertical = 4.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        AnimatedVisibility(visible = !direct, enter = editorEnter, exit = editorExit) {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Box(Modifier.fillMaxWidth().onGloballyPositioned { coordinates ->
                    editorWidth = coordinates.size.width
                    historyRoom = with(density) {
                        (coordinates.positionInWindow().y.toInt() - topInset).coerceAtLeast(0).toDp()
                    } - 8.dp
                }) {
                    Column(
                        Modifier.fillMaxWidth()
                            .animateContentSize(motion.contentSizeSpec())
                            .clip(RoundedCornerShape(18.dp))
                            .background(colors.surfaceVariant.copy(alpha = .42f))
                            .padding(4.dp),
                    ) {
                        Box(
                            Modifier.fillMaxWidth()
                                .heightIn(min = 48.dp, max = if (editorExpanded) 208.dp else 96.dp),
                        ) {
                            BasicTextField(
                                value = draft,
                                onValueChange = { value -> if (value.length <= 8192) updateDraft(value) },
                                modifier = Modifier.fillMaxWidth()
                                    .heightIn(min = 48.dp, max = if (editorExpanded) 208.dp else 96.dp)
                                    .focusRequester(focusRequester)
                                    .padding(start = 8.dp, end = 42.dp, top = 12.dp, bottom = 12.dp),
                                maxLines = if (editorExpanded) 8 else 3,
                                interactionSource = editorInteractions,
                                textStyle = TextStyle(
                                    color = colors.onSurface,
                                    fontSize = 15.sp,
                                    lineHeight = 21.sp,
                                    fontFamily = LocalTerminalFont.current,
                                ),
                                cursorBrush = SolidColor(colors.primary),
                                keyboardOptions = KeyboardOptions(
                                    capitalization = KeyboardCapitalization.None,
                                    autoCorrectEnabled = false,
                                ),
                                decorationBox = { inner ->
                                    if (draft.isEmpty()) HelperText(stringResource(R.string.local_edit))
                                    inner()
                                },
                            )
                            ComposerIconButton(
                                icon = if (editorExpanded) R.drawable.ic_down else R.drawable.ic_expand,
                                label = stringResource(
                                    if (editorExpanded) R.string.composer_collapse_editor else R.string.composer_expand_editor,
                                ),
                                enabled = true,
                                modifier = Modifier.align(Alignment.TopEnd),
                                onClick = { editorExpanded = !editorExpanded },
                            )
                        }
                        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                            onAttach?.let { attach ->
                                ComposerIconButton(
                                    icon = R.drawable.ic_plus,
                                    label = stringResource(R.string.composer_attach),
                                    enabled = enabled && !attachmentBusy,
                                    busy = attachmentBusy,
                                    onClick = attach,
                                )
                            }
                            Spacer(Modifier.weight(1f))
                            ComposerIconButton(
                                icon = R.drawable.ic_microphone,
                                label = stringResource(R.string.composer_voice),
                                enabled = enabled && !sending,
                                onClick = ::launchVoice,
                            )
                            ComposerIconButton(
                                icon = R.drawable.ic_send,
                                label = stringResource(R.string.send),
                                enabled = enabled && !sending && draft.isNotBlank(),
                                selected = true,
                                selectedContainer = colors.primary,
                                tint = colors.onPrimary,
                                onClick = ::requestSend,
                            )
                        }
                    }
                    if (showHistory) {
                        ComposerHistory(
                            commands = historyItems,
                            selected = selectedHistory,
                            modifier = Modifier.width(with(density) { editorWidth.toDp() }),
                            maxHeight = minOf(208.dp, historyRoom),
                            onDismiss = { historyDismissed = true },
                            onSelect = { command ->
                                selectedHistory = command
                                historyDismissed = true
                                repository.setDraft(id, command)
                                focusRequester.requestFocus()
                            },
                        )
                    }
                }
                if (voiceFeedback != VoiceFeedback.None) {
                    Text(
                        stringResource(
                            when (voiceFeedback) {
                                VoiceFeedback.Unavailable -> R.string.composer_voice_unavailable
                                VoiceFeedback.Failed -> R.string.composer_voice_failed
                                VoiceFeedback.Empty -> R.string.composer_voice_empty
                                VoiceFeedback.TooLong -> R.string.composer_voice_too_long
                                VoiceFeedback.None -> R.string.composer_voice_failed
                            },
                        ),
                        modifier = Modifier.padding(start = 12.dp),
                        color = colors.error,
                        fontSize = 12.sp,
                        lineHeight = 18.sp,
                    )
                }
            }
        }

        if (!direct && keys && onKey != null) {
            ComposerShortcutRow(shortcuts, enabled, onKey)
        }
        ComposerToolbar(
            direct = direct,
            onDirect = onDirect,
            keys = keys,
            onToggleKeys = if (onKey != null && !direct) ({ keys = !keys }) else null,
            focused = focused,
            onToggleFocus = if (!direct) onToggleFocus else null,
            keyboardVisible = keyboardVisible,
            keyboardEnabled = !direct || onKeyboard != null,
            enabled = enabled,
            shortcuts = if (direct) shortcuts else emptyList(),
            onKey = if (direct) onKey else null,
            onImeToggle = {
                if (keyboardVisible) keyboard?.hide()
                else if (direct) onKeyboard?.invoke()
                else {
                    focusRequester.requestFocus()
                    keyboard?.show()
                }
            },
        )
    }

}
