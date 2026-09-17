package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
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
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.session.SessionRepository
import io.github.kuddev.pebrel.mobile.voice.appendVoiceDraft
import kotlinx.coroutines.launch

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
    val focusManager = LocalFocusManager.current
    val density = LocalDensity.current
    val keyboardVisible = WindowInsets.ime.getBottom(density) > 0
    val topInset = WindowInsets.statusBars.getTop(density)
    val motion = rememberPebrelMotion()
    val colors = MaterialTheme.colorScheme
    val editorInteractions = remember(id) { MutableInteractionSource() }
    val editorFocused by editorInteractions.collectIsFocusedAsState()
    var sending by remember(id) { mutableStateOf(false) }
    var keys by rememberSaveable(id) { mutableStateOf(false) }
    var editorExpanded by rememberSaveable(id) { mutableStateOf(false) }
    var selectedHistory by rememberSaveable(id) { mutableStateOf<String?>(null) }
    var voiceTooLong by remember(id) { mutableStateOf(false) }
    var historyDismissed by rememberSaveable(id) { mutableStateOf(false) }
    var editorWidth by remember { mutableIntStateOf(0) }
    var historyRoom by remember { mutableStateOf(0.dp) }
    var toolsOpen by remember(id, direct) { mutableStateOf(false) }

    val voice = rememberComposerVoice(id, enabled && !direct && !sending) { spoken ->
        val next = appendVoiceDraft(repository.drafts.value[id].orEmpty(), spoken)
        voiceTooLong = next == null
        if (next != null) {
            selectedHistory = null
            repository.setDraft(id, next)
        }
    }
    val voiceState by voice.state.collectAsStateWithLifecycle()

    fun updateDraft(value: String) {
        selectedHistory = null
        historyDismissed = false
        repository.setDraft(id, value)
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

    LaunchedEffect(id, direct) {
        if (!direct && onDirect != null) {
            focusRequester.requestFocus()
            keyboard?.show()
        }
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
    // One mutually exclusive slot. Animating two independently visible surfaces
    // left an empty toolbar under the editor and retained two input targets.
    Column(Modifier.fillMaxWidth().padding(horizontal = 6.dp, vertical = 4.dp)
        .animateContentSize(motion.contentSizeSpec())) {
        if (direct) {
            ComposerToolbar(
                onEdit = onDirect?.let { toggle -> { toggle(false) } },
                keyboardVisible = keyboardVisible,
                keyboardEnabled = onKeyboard != null,
                enabled = enabled,
                shortcuts = shortcuts,
                onKey = onKey,
                onImeToggle = {
                    if (keyboardVisible) keyboard?.hide() else onKeyboard?.invoke()
                },
            )
        } else {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Box(Modifier.fillMaxWidth().onGloballyPositioned { coordinates ->
                    editorWidth = coordinates.size.width
                    historyRoom = with(density) {
                        (coordinates.positionInWindow().y.toInt() - topInset).coerceAtLeast(0).toDp()
                    } - 8.dp
                }) {
                    Column(
                        Modifier.fillMaxWidth().testTag("composer-editor")
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
                            onDirect?.let { toggle ->
                                ComposerIconButton(
                                    icon = R.drawable.ic_terminal,
                                    label = stringResource(R.string.composer_mode_direct),
                                    onClick = {
                                        focusManager.clearFocus()
                                        toggle(true)
                                    },
                                )
                            }
                            Box {
                                ComposerIconButton(
                                    icon = R.drawable.ic_plus,
                                    label = stringResource(R.string.composer_tools),
                                    busy = attachmentBusy,
                                    onClick = { toolsOpen = true },
                                )
                                DropdownMenu(toolsOpen, { toolsOpen = false }) {
                                    onAttach?.let { attach ->
                                        DropdownMenuItem(
                                            text = { Text(stringResource(R.string.composer_attach)) },
                                            enabled = enabled && !attachmentBusy,
                                            onClick = { toolsOpen = false; attach() },
                                        )
                                    }
                                    if (onKey != null) DropdownMenuItem(
                                        text = { Text(stringResource(if (keys) R.string.composer_hide_aux_keys else R.string.composer_aux_keys)) },
                                        onClick = { toolsOpen = false; keys = !keys },
                                    )
                                    onToggleFocus?.let { toggle ->
                                        DropdownMenuItem(
                                            text = { Text(stringResource(if (focused) R.string.composer_exit_focus else R.string.composer_focus)) },
                                            onClick = { toolsOpen = false; toggle() },
                                        )
                                    }
                                    DropdownMenuItem(
                                        text = { Text(stringResource(if (keyboardVisible) R.string.composer_hide_keyboard else R.string.composer_show_keyboard)) },
                                        onClick = {
                                            toolsOpen = false
                                            if (keyboardVisible) keyboard?.hide() else {
                                                focusRequester.requestFocus()
                                                keyboard?.show()
                                            }
                                        },
                                    )
                                }
                            }
                            Spacer(Modifier.weight(1f))
                            ComposerVoiceButton(voice, voiceState, enabled && !sending)
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
                        if (keys && onKey != null) {
                            ComposerShortcutRow(shortcuts, enabled, onKey, surface = false)
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
                voiceState.error?.let { VoiceErrorText(it) }
                if (voiceTooLong) {
                    Text(
                        stringResource(R.string.composer_voice_too_long),
                        modifier = Modifier.padding(start = 12.dp),
                        color = colors.error,
                        fontSize = 12.sp,
                        lineHeight = 18.sp,
                    )
                }
            }
        }

    }

}
