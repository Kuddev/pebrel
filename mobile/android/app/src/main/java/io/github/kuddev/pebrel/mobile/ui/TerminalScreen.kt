package io.github.kuddev.pebrel.mobile.ui

import android.view.KeyEvent
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.DesktopPane
import io.github.kuddev.pebrel.mobile.session.DesktopWorkspace
import io.github.kuddev.pebrel.mobile.session.LocalSession
import io.github.kuddev.pebrel.mobile.session.SessionRepository
import kotlinx.coroutines.delay

@Composable
fun LocalTerminalScreen(session: LocalSession, repository: SessionRepository, onBack: () -> Unit, onSessions: () -> Unit,
                        onRetry: () -> Unit, onEdit: () -> Unit, onClose: () -> Unit) {
    val prefs by repository.display.state.collectAsStateWithLifecycle()
    var direct by rememberSaveable(session.id) { mutableStateOf(prefs.directInput) }
    var closing by remember { mutableStateOf(false) }
    if (session.host != null && (session.status == "connecting" || (session.status == "failed" && session.terminal.frame == null))) {
        SshConnectionStatus(session, onClose, onRetry, onEdit)
        return
    }
    TerminalHeader(session.title, session.source, session.status, onBack, onSessions, { closing = true })
    TerminalContext(if (session.source == "Local") stringResource(R.string.local_device) else session.source, stringResource(R.string.raw_terminal))
    Column(Modifier.fillMaxSize()) {
        if (session.status in setOf("ended", "failed")) TerminalDisconnected(session, if (session.host != null) onRetry else null)
        if (session.status == "connecting") LinearProgressIndicator(Modifier.fillMaxWidth())
        key(session.id) {
            TerminalSurface(session, repository, Modifier.weight(1f).fillMaxWidth().padding(horizontal = 8.dp)
                .workspaceFrame().padding(5.dp), direct, prefs.fontSize)
        }
        CommandComposer(session.id, repository, session.status == "ready", direct, { direct = it }, { label ->
            val key = when (label) {
                "Ctrl+C" -> KeyEvent.KEYCODE_C
                "Esc" -> KeyEvent.KEYCODE_ESCAPE
                "Tab" -> KeyEvent.KEYCODE_TAB
                "←" -> KeyEvent.KEYCODE_DPAD_LEFT
                "→" -> KeyEvent.KEYCODE_DPAD_RIGHT
                "↑" -> KeyEvent.KEYCODE_DPAD_UP
                else -> KeyEvent.KEYCODE_DPAD_DOWN
            }
            if (!session.terminal.key(key, if (label == "Ctrl+C") 2 else 0,
                    text = if (label == "Ctrl+C") "c" else "", unshifted = if (label == "Ctrl+C") 99 else 0)) repository.error.value = "input_rejected"
        }) { command ->
            val bytes = (command + "\r").toByteArray()
            session.terminal.tryWrite(bytes, 0, bytes.size)
        }
    }
    if (closing) AlertDialog(onDismissRequest = { closing = false }, title = { Text(stringResource(R.string.close_session)) },
        text = { Text(stringResource(R.string.close_session_confirm, session.title)) },
        confirmButton = { TextButton(onClose) { Text(stringResource(R.string.close_session)) } },
        dismissButton = { TextButton({ closing = false }) { Text(stringResource(R.string.cancel)) } })
}

@Composable
fun DesktopTerminalScreen(desktop: DesktopWorkspace, pane: DesktopPane, repository: SessionRepository, onBack: () -> Unit, onSessions: () -> Unit) {
    val output by repository.output.collectAsStateWithLifecycle()
    val prefs by repository.display.state.collectAsStateWithLifecycle()
    val lifecycle = LocalLifecycleOwner.current
    val identity = "${desktop.id}:${pane.window}:${pane.id}"
    LaunchedEffect(identity, desktop.status) {
        if (desktop.status == "ready") lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) { repository.readDesktop(desktop.id, pane); delay(2000) }
        }
    }
    DisposableEffect(identity) { onDispose { repository.leaveDesktopPane() } }
    TerminalHeader(pane.title, desktop.host.name, desktop.status, onBack, onSessions)
    TerminalContext(pane.cwd.ifBlank { pane.task }, stringResource(R.string.recent_output))
    Column(Modifier.fillMaxSize()) {
        if (desktop.status != "ready") HelperText(stringResource(R.string.device_unavailable), Modifier.padding(horizontal = 22.dp, vertical = 8.dp))
        SelectionContainer(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState())) {
            Text(if (output.target == identity) output.text else "", fontFamily = LocalTerminalFont.current,
                fontSize = prefs.fontSize.sp, modifier = Modifier.fillMaxWidth().padding(horizontal = 22.dp, vertical = 12.dp))
        }
        if (output.loading && output.text.isBlank()) LinearProgressIndicator(Modifier.fillMaxWidth())
        CommandComposer(identity, repository, desktop.allowInput && desktop.status == "ready", false, null, null) { command ->
            repository.sendDesktop(desktop.id, pane, command)
        }
    }
}

@Composable
private fun TerminalHeader(title: String, endpoint: String, status: String, onBack: () -> Unit, onSessions: () -> Unit, onClose: (() -> Unit)? = null) {
    var menu by remember { mutableStateOf(false) }
    Row(Modifier.fillMaxWidth().height(72.dp).background(MaterialTheme.colorScheme.surface).padding(horizontal = 9.dp), verticalAlignment = Alignment.CenterVertically) {
        GlyphButton(R.drawable.ic_back, stringResource(R.string.back), onBack)
        Column(Modifier.weight(1f).clickable(onClickLabel = stringResource(R.string.switch_session), onClick = onSessions).padding(horizontal = 8.dp, vertical = 9.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                Text(title, fontSize = 16.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
                Glyph(R.drawable.ic_down, Modifier.size(13.dp))
            }
            Box(Modifier.padding(top = 5.dp)) { StatusCaption(status, "$endpoint · ") }
        }
        if (onClose != null) Box {
            GlyphButton(R.drawable.ic_more, stringResource(R.string.more_actions), { menu = true })
            DropdownMenu(menu, { menu = false }) {
                DropdownMenuItem(text = { Text(stringResource(R.string.close_session)) }, onClick = { menu = false; onClose() })
            }
        }
    }
}

@Composable
private fun TerminalContext(context: String, view: String) {
    Row(Modifier.fillMaxWidth().heightIn(min = 42.dp).padding(horizontal = 22.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(context, fontSize = 11.sp, color = MaterialTheme.colorScheme.onSurfaceVariant, fontFamily = LocalTerminalFont.current,
            maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
        Text(view, fontSize = 11.sp, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(start = 12.dp))
    }
}

@Composable
fun DesktopScreen(desktop: DesktopWorkspace?, onPane: (DesktopPane) -> Unit, onDisconnect: () -> Unit) {
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 22.dp, vertical = 12.dp)) {
        Row(Modifier.padding(vertical = 20.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(15.dp)) {
            Glyph(R.drawable.ic_monitor, Modifier.size(30.dp), MaterialTheme.colorScheme.primary)
            Column {
                Text(desktop?.host?.name.orEmpty(), fontSize = 19.sp)
                Box(Modifier.padding(top = 7.dp)) { StatusCaption(desktop?.status ?: "disconnected", "${desktop?.transport.orEmpty()} · ") }
            }
        }
        if (desktop?.status == "connecting") LinearProgressIndicator(Modifier.fillMaxWidth())
        GroupHeading(stringResource(R.string.computer_tabs), desktop?.panes?.size ?: 0)
        desktop?.panes?.forEach { pane ->
            Row(Modifier.fillMaxWidth().padding(bottom = 10.dp).workspaceFrame()
                .clickable(enabled = desktop.status == "ready") { onPane(pane) }.padding(horizontal = 14.dp, vertical = 18.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Glyph(R.drawable.ic_terminal)
                Column(Modifier.weight(1f)) {
                    Text(pane.title, fontSize = 14.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(pane.task.ifBlank { pane.cwd }, fontSize = 11.sp, fontFamily = LocalTerminalFont.current,
                        color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(top = 6.dp))
                }
                Text(statusLabel(pane.state), fontSize = 10.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        if (desktop?.panes.isNullOrEmpty()) HelperText(stringResource(R.string.no_panes), Modifier.padding(vertical = 20.dp))
        if (desktop?.status !in listOf("ready", "connecting")) HelperText(stringResource(R.string.device_unavailable), Modifier.padding(vertical = 12.dp))
        OutlinedButton(onDisconnect, modifier = Modifier.padding(top = 20.dp)) { Text(stringResource(R.string.disconnect)) }
    }
}
