package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.animation.expandVertically
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.session.SessionRepository
import kotlinx.coroutines.launch

@Composable
fun CommandComposer(
    id: String, repository: SessionRepository, enabled: Boolean, direct: Boolean, onDirect: ((Boolean) -> Unit)?,
    onKey: ((String) -> Unit)?, onKeyboard: (() -> Unit)? = null,
    focused: Boolean = false, onToggleFocus: (() -> Unit)? = null, send: suspend (String) -> Boolean,
) {
    val drafts by repository.drafts.collectAsStateWithLifecycle()
    val preferences by repository.display.state.collectAsStateWithLifecycle()
    val draft = drafts[id].orEmpty()
    val scope = rememberCoroutineScope()
    val focusRequester = remember { FocusRequester() }
    val keyboard = LocalSoftwareKeyboardController.current
    val keyboardVisible = WindowInsets.ime.getBottom(LocalDensity.current) > 0
    val motion = rememberPebrelMotion()
    var sending by remember(id) { mutableStateOf(false) }
    var keys by rememberSaveable(id) { mutableStateOf(false) }
    var editor by rememberSaveable(id) { mutableStateOf(false) }
    val suggestions = remember(draft, preferences.suggestions, direct) {
        if (!preferences.suggestions || direct || draft.isBlank()) emptyList() else
            listOf("git status", "git diff", "git log --oneline -5", "pwd", "ls").filter { it.startsWith(draft) && it != draft }
    }
    Column(Modifier.fillMaxWidth().padding(horizontal = 6.dp, vertical = 4.dp)) {
        if (suggestions.isNotEmpty()) {
            HelperText(stringResource(R.string.completion_hint))
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                suggestions.forEach { value ->
                    TextButton(onClick = { repository.setDraft(id, value) }) { Text(value, fontFamily = LocalTerminalFont.current, fontSize = 12.sp) }
                }
            }
        }
        AnimatedVisibility(!direct,
            enter = if (motion.animationsEnabled) expandVertically() + fadeIn() else EnterTransition.None,
            exit = if (motion.animationsEnabled) shrinkVertically() + fadeOut() else ExitTransition.None) {
            Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).background(MaterialTheme.colorScheme.secondaryContainer.copy(alpha = .45f), RoundedCornerShape(18.dp))
                .padding(start = 13.dp, end = 5.dp, top = 5.dp, bottom = 5.dp), verticalAlignment = Alignment.CenterVertically) {
                Text("❯", color = MaterialTheme.colorScheme.primary, fontSize = 15.sp)
                BasicTextField(draft, { if (it.length <= 8192) repository.setDraft(id, it) },
                    modifier = Modifier.weight(1f).focusRequester(focusRequester).padding(horizontal = 10.dp, vertical = 7.dp), maxLines = 4,
                    textStyle = TextStyle(color = MaterialTheme.colorScheme.onSurface, fontSize = 15.sp, fontFamily = LocalTerminalFont.current),
                    cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false),
                    decorationBox = { inner -> Box { if (draft.isEmpty()) HelperText(stringResource(R.string.local_edit)); inner() } })
                FilledIconButton(onClick = {
                    val submitted = draft
                    sending = true
                    scope.launch {
                        try {
                            if (send(submitted)) repository.acknowledgeDraft(id, submitted)
                            else if (repository.error.value == null) repository.error.value = "input_rejected"
                        } finally { sending = false }
                    }
                }, enabled = enabled && !sending && draft.isNotBlank(), shape = RoundedCornerShape(50), modifier = Modifier.size(40.dp)) {
                    Icon(androidx.compose.ui.res.painterResource(R.drawable.ic_up), stringResource(R.string.send), Modifier.size(20.dp))
                }
            }
        }
        Row(Modifier.fillMaxWidth().heightIn(min = 48.dp)
            .background(MaterialTheme.colorScheme.secondaryContainer.copy(alpha = .3f), RoundedCornerShape(26.dp))
            .padding(horizontal = 2.dp), verticalAlignment = Alignment.CenterVertically) {
            if ((direct || keys) && onKey != null) {
                Row(Modifier.weight(1f).horizontalScroll(rememberScrollState())) {
                    listOf("Ctrl+C", "Esc", "Tab", "←", "→", "↑", "↓").forEach { key ->
                        TextButton(onClick = { onKey(key) }, enabled = enabled, shape = RoundedCornerShape(50),
                            contentPadding = PaddingValues(horizontal = 9.dp), modifier = Modifier.heightIn(min = 48.dp)) {
                            Text(key, fontSize = 12.sp, fontFamily = LocalTerminalFont.current)
                        }
                    }
                }
            } else {
                if (onKey != null) GlyphButton(R.drawable.ic_command, stringResource(R.string.extra_keys), { keys = !keys })
                Spacer(Modifier.weight(1f))
            }
            if (onDirect != null) GlyphButton(if (direct) R.drawable.ic_edit else R.drawable.ic_terminal,
                stringResource(if (direct) R.string.local_compose else R.string.direct_input), { onDirect(!direct) })
            if (!direct) GlyphButton(R.drawable.ic_expand, stringResource(R.string.long_editor), { editor = true })
            if (onToggleFocus != null) GlyphButton(if (focused) R.drawable.ic_down else R.drawable.ic_expand,
                stringResource(if (focused) R.string.exit_terminal_focus else R.string.enter_terminal_focus), onToggleFocus)
            GlyphButton(R.drawable.ic_keyboard, stringResource(R.string.toggle_keyboard), {
                if (keyboardVisible) keyboard?.hide()
                else if (direct) onKeyboard?.invoke()
                else { focusRequester.requestFocus(); keyboard?.show() }
            }, enabled = enabled)
        }
    }
    if (editor) Dialog(onDismissRequest = { editor = false }, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            Column(Modifier.systemBarsPadding().imePadding()) {
                PageHeader(stringResource(R.string.long_editor), { editor = false }) {
                    TextButton({ editor = false }) { Text(stringResource(R.string.editor_done)) }
                }
                OutlinedTextField(draft, { if (it.length <= 8192) repository.setDraft(id, it) },
                    modifier = Modifier.weight(1f).fillMaxWidth().padding(18.dp), textStyle = TextStyle(fontFamily = LocalTerminalFont.current, fontSize = 16.sp),
                    keyboardOptions = KeyboardOptions(autoCorrectEnabled = false), placeholder = { Text(stringResource(R.string.local_edit)) })
            }
        }
    }
}
