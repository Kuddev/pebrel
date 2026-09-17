package io.github.kuddev.pebrel.mobile.ui

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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.selected
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
    onKey: ((String) -> Unit)?, send: suspend (String) -> Boolean,
) {
    val drafts by repository.drafts.collectAsStateWithLifecycle()
    val preferences by repository.display.state.collectAsStateWithLifecycle()
    val draft = drafts[id].orEmpty()
    val scope = rememberCoroutineScope()
    var sending by remember(id) { mutableStateOf(false) }
    var keys by rememberSaveable(id) { mutableStateOf(false) }
    var editor by rememberSaveable(id) { mutableStateOf(false) }
    val suggestions = remember(draft, preferences.suggestions, direct) {
        if (!preferences.suggestions || direct || draft.isBlank()) emptyList() else
            listOf("git status", "git diff", "git log --oneline -5", "pwd", "ls").filter { it.startsWith(draft) && it != draft }
    }
    HorizontalDivider(thickness = .5.dp)
    Column(Modifier.fillMaxWidth().padding(start = 18.dp, end = 18.dp, top = 11.dp)) {
        if (suggestions.isNotEmpty()) {
            HelperText(stringResource(R.string.completion_hint))
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                suggestions.forEach { value ->
                    TextButton(onClick = { repository.setDraft(id, value) }) { Text(value, fontFamily = LocalTerminalFont.current, fontSize = 12.sp) }
                }
            }
        }
        if (direct) HelperText(stringResource(if (enabled) R.string.direct_hint else R.string.input_unavailable), Modifier.padding(vertical = 12.dp))
        else Row(Modifier.fillMaxWidth().heightIn(min = 58.dp).background(MaterialTheme.colorScheme.secondaryContainer.copy(alpha = .64f), RoundedCornerShape(5.dp))
            .padding(start = 13.dp, end = 5.dp, top = 5.dp, bottom = 5.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("❯", color = MaterialTheme.colorScheme.primary, fontSize = 15.sp)
            BasicTextField(draft, { if (it.length <= 8192) repository.setDraft(id, it) },
                modifier = Modifier.weight(1f).padding(horizontal = 10.dp, vertical = 7.dp), maxLines = 4,
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
            }, enabled = enabled && !sending && draft.isNotBlank(), shape = RoundedCornerShape(4.dp), modifier = Modifier.size(48.dp)) {
                Icon(androidx.compose.ui.res.painterResource(R.drawable.ic_up), stringResource(R.string.send), Modifier.size(20.dp))
            }
        }
        Row(Modifier.fillMaxWidth().heightIn(min = 54.dp), verticalAlignment = Alignment.CenterVertically) {
            if (onDirect != null) {
                Row(Modifier.background(MaterialTheme.colorScheme.secondaryContainer.copy(alpha = .3f), RoundedCornerShape(4.dp))) {
                    ModeButton(stringResource(R.string.local_compose), !direct) { onDirect(false) }
                    ModeButton(stringResource(R.string.direct_input), direct) { onDirect(true) }
                }
            } else HelperText(stringResource(if (enabled) R.string.local_compose else R.string.read_only))
            Spacer(Modifier.weight(1f))
            if (!direct) GlyphButton(R.drawable.ic_expand, stringResource(R.string.long_editor), { editor = true })
            if (onKey != null) GlyphButton(R.drawable.ic_keyboard, stringResource(R.string.extra_keys), { keys = !keys })
        }
        if ((keys || direct) && onKey != null) Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            listOf("Esc", "Tab", "Ctrl+C", "←", "→", "↑", "↓").forEach { key ->
                TextButton(onClick = { onKey(key) }, enabled = enabled, contentPadding = PaddingValues(horizontal = 10.dp), modifier = Modifier.heightIn(min = 48.dp)) {
                    Text(key, fontSize = 12.sp, fontFamily = LocalTerminalFont.current)
                }
            }
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

@Composable
private fun ModeButton(label: String, selected: Boolean, onClick: () -> Unit) {
    TextButton(onClick, shape = RoundedCornerShape(4.dp), contentPadding = PaddingValues(horizontal = 9.dp),
        colors = ButtonDefaults.textButtonColors(containerColor = if (selected) MaterialTheme.colorScheme.secondaryContainer else androidx.compose.ui.graphics.Color.Transparent,
            contentColor = if (selected) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant),
        modifier = Modifier.heightIn(min = 48.dp).semantics { this.selected = selected }) {
        Text(label, fontSize = 11.sp)
    }
}
