package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.session.ShortcutPreferences
import io.github.kuddev.pebrel.terminal.TerminalShortcut

private val modifierKeys = listOf("Ctrl", "Alt", "Shift", "Meta")
private val commonKeys = listOf("Esc", "Tab", "Enter", "Space", "Backspace", "Delete", "Insert", "Home", "End", "PageUp", "PageDown",
    "←", "→", "↑", "↓")
private val functionKeys = (1..12).map { "F$it" }

/** Keys are composed by tapping, so nobody has to remember the syntax on a phone keyboard. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun ShortcutEditor(preferences: ShortcutPreferences, onDismiss: () -> Unit) {
    val saved by preferences.state.collectAsStateWithLifecycle()
    var stored by rememberSaveable { mutableStateOf(saved.joinToString("\n")) }
    val keys = stored.lines().filter(String::isNotEmpty)
    fun update(next: List<String>) { stored = next.joinToString("\n") }
    var held by rememberSaveable { mutableStateOf("") }
    var key by rememberSaveable { mutableStateOf("") }
    var functions by rememberSaveable { mutableStateOf(false) }
    val modifiers = held.split(' ').filter(String::isNotEmpty)
    val candidate = key.takeIf(String::isNotEmpty)?.let { TerminalShortcut.parse((modifiers + it).joinToString("+")) }
    val duplicate = candidate != null && candidate.label in keys
    val full = keys.size >= ShortcutPreferences.MAX_KEYS
    ConnectionForm(stringResource(R.string.custom_shortcuts), onDismiss, footer = {
        Button({ if (preferences.save(keys)) onDismiss() }, Modifier.fillMaxWidth().heightIn(min = 48.dp),
            shape = MaterialTheme.shapes.medium) { Text(stringResource(R.string.save)) }
    }) {
        Column(verticalArrangement = Arrangement.spacedBy(20.dp)) {
            FormSection(stringResource(R.string.custom_shortcuts_toolbar)) {
                if (keys.isEmpty()) HelperText(stringResource(R.string.custom_shortcuts_empty))
                keys.forEachIndexed { index, label ->
                    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.weight(1f)) {
                            Text(label, Modifier.clip(RoundedCornerShape(8.dp)).background(MaterialTheme.colorScheme.surface)
                                .padding(horizontal = 10.dp, vertical = 6.dp),
                                fontFamily = LocalTerminalFont.current, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                        fun swap(other: Int) = update(keys.toMutableList().apply { this[index] = keys[other]; this[other] = label })
                        GlyphButton(R.drawable.ic_up, stringResource(R.string.custom_shortcuts_move_up), { swap(index - 1) }, enabled = index > 0)
                        GlyphButton(R.drawable.ic_down, stringResource(R.string.custom_shortcuts_move_down), { swap(index + 1) }, enabled = index < keys.lastIndex)
                        GlyphButton(R.drawable.ic_close, stringResource(R.string.custom_shortcuts_remove, label), { update(keys - label) })
                    }
                }
            }
            FormSection(stringResource(R.string.custom_shortcuts_new)) {
                if (duplicate) Text(stringResource(R.string.custom_shortcuts_duplicate), color = MaterialTheme.colorScheme.error, fontSize = 12.sp)
                if (full) Text(stringResource(R.string.custom_shortcuts_limit, ShortcutPreferences.MAX_KEYS), color = MaterialTheme.colorScheme.error, fontSize = 12.sp)
                FilledTonalButton({
                    update(keys + checkNotNull(candidate).label); held = ""; key = ""
                }, Modifier.fillMaxWidth().heightIn(min = 48.dp), enabled = candidate != null && !duplicate && !full,
                    shape = MaterialTheme.shapes.medium) {
                    Text(candidate?.let { stringResource(R.string.custom_shortcuts_add, it.label) } ?: stringResource(R.string.custom_shortcuts_pick),
                        maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    modifierKeys.forEach { name ->
                        FilterChip(name in modifiers, {
                            held = (if (name in modifiers) modifiers - name else modifiers + name).joinToString(" ")
                        }, { Text(name) })
                    }
                }
                ConnectionField(key.takeIf { it.length == 1 && it[0].isLetterOrDigit() }.orEmpty(),
                    { key = it.takeLast(1) }, R.string.custom_shortcuts_character, limit = 2)
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    (commonKeys + if (functions) functionKeys else emptyList()).forEach { name ->
                        FilterChip(key == name, { key = if (key == name) "" else name }, { Text(name) })
                    }
                    FilterChip(functions, { functions = !functions }, { Text("F1–F12") })
                }
            }
            HelperText(stringResource(R.string.custom_shortcuts_hint))
        }
    }
}
