package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.session.ShortcutPreferences
import io.github.kuddev.pebrel.terminal.TerminalShortcut

/** One definition per line keeps editing, ordering and deletion usable with a phone keyboard. */
@Composable
internal fun ShortcutEditor(preferences: ShortcutPreferences, onDismiss: () -> Unit) {
    val saved by preferences.state.collectAsStateWithLifecycle()
    var input by rememberSaveable { mutableStateOf(saved.joinToString("\n")) }
    val lines = input.lines().map(String::trim).filter(String::isNotEmpty)
    val parsed = lines.map(TerminalShortcut::parse)
    val invalid = parsed.indexOfFirst { it == null }
    val duplicates = parsed.filterNotNull().map { it.label }.let { it.distinct().size != it.size }
    val tooMany = lines.size > ShortcutPreferences.MAX_KEYS
    val valid = invalid < 0 && !duplicates && !tooMany
    AlertDialog(onDismissRequest = onDismiss, title = { Text(stringResource(R.string.custom_shortcuts)) },
        text = { Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            HelperText(stringResource(R.string.custom_shortcuts_hint))
            OutlinedTextField(input, { if (it.length <= 2048) input = it },
                label = { Text(stringResource(R.string.custom_shortcuts)) },
                modifier = Modifier.fillMaxWidth(), minLines = 3, maxLines = 8,
                isError = !valid, supportingText = {
                    when {
                        invalid >= 0 -> Text(stringResource(R.string.custom_shortcuts_invalid, invalid + 1))
                        duplicates -> Text(stringResource(R.string.custom_shortcuts_duplicate))
                        tooMany -> Text(stringResource(R.string.custom_shortcuts_limit, ShortcutPreferences.MAX_KEYS))
                    }
                })
            if (valid && parsed.isNotEmpty()) Text(parsed.filterNotNull().joinToString(" · ") { it.label }, fontFamily = LocalTerminalFont.current)
        } },
        confirmButton = { TextButton({ if (preferences.save(lines)) onDismiss() }, enabled = valid) { Text(stringResource(R.string.save)) } },
        dismissButton = { TextButton(onDismiss) { Text(stringResource(R.string.cancel)) } })
}
