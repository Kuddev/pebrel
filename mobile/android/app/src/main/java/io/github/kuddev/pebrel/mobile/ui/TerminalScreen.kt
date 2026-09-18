package io.github.kuddev.pebrel.mobile.ui

import android.view.KeyEvent
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
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
    var direct by rememberSaveable(session.id, prefs.directInput) { mutableStateOf(prefs.directInput) }
    val attachments = rememberTerminalAttachmentAction(session, repository) { direct = false }
    var closing by remember { mutableStateOf(false) }
    var focused by rememberSaveable(session.id) { mutableStateOf(false) }
    var keyboardRequest by remember(session.id) { mutableIntStateOf(0) }
    val trust by repository.trust.collectAsStateWithLifecycle()
    if (!focused) TerminalHeader(session.title,
        if (session.source == "Local") stringResource(R.string.local_device) else session.source,
        session.status, onBack, onSessions, { closing = true })
    Column(Modifier.fillMaxSize()) {
        if (session.status == "ended" || (session.status == "failed" && session.hasConnected)) TerminalDisconnected(session, if (session.host != null) onRetry else null)
        Box(Modifier.weight(1f).fillMaxWidth()) {
            key(session.id) {
                TerminalSurface(session, repository, Modifier.fillMaxSize(), direct, prefs.fontSize, keyboardRequest)
            }
            if (session.host != null && (session.status == "connecting" || (session.status == "failed" && !session.hasConnected))) {
                SshConnectionStatus(session, onClose, onRetry, onEdit,
                    trust = trust?.takeIf { it.ownerId == session.id }, onTrust = repository::answerTrust)
            }
        }
        CommandComposer(session.id, repository, session.status == "ready", direct, {
            direct = it
            if (it) keyboardRequest++
        }, { label ->
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
        }, onKeyboard = { keyboardRequest++ }, focused = focused, onToggleFocus = { focused = !focused },
            onAttach = attachments.pick, attachmentBusy = attachments.busy) { command ->
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
    var direct by rememberSaveable(identity, prefs.directInput) { mutableStateOf(prefs.directInput) }
    var keyboardRequest by remember(identity) { mutableIntStateOf(0) }
    var focused by rememberSaveable(identity) { mutableStateOf(false) }
    val enabled = desktop.allowInput && desktop.status == "ready"
    val keyboardVisible = WindowInsets.ime.getBottom(LocalDensity.current) > 0
    val input = remember(identity, enabled) { repository.desktopInput(desktop.id, pane) }
    DisposableEffect(input) { onDispose { input.close() } }
    LaunchedEffect(identity, desktop.status, direct, keyboardVisible) {
        if (desktop.status == "ready") lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) { repository.readDesktop(desktop.id, pane); delay(if (enabled && direct && keyboardVisible) 250 else 2000) }
        }
    }
    DisposableEffect(identity) { onDispose { repository.leaveDesktopPane() } }
    if (!focused) TerminalHeader(pane.title, desktop.host.name, desktop.status, onBack, onSessions)
    Column(Modifier.fillMaxSize()) {
        if (desktop.status != "ready") HelperText(stringResource(R.string.device_unavailable), Modifier.padding(horizontal = 22.dp, vertical = 8.dp))
        else if (!desktop.allowInput) HelperText(stringResource(R.string.composer_pc_read_only), Modifier.padding(horizontal = 12.dp, vertical = 4.dp))
        DesktopOutputSurface(identity, if (output.target == identity) output.text else "", prefs.fontSize,
            prefs.pinchZoom, { size -> repository.display.update { it.copy(fontSize = size) } },
            Modifier.weight(1f).fillMaxWidth(), frame = if (output.target == identity) output.frame else null,
            inputTarget = input.takeIf { enabled && direct }, keyboardRequest = keyboardRequest)
        if (output.loading && output.text.isBlank()) LinearProgressIndicator(Modifier.fillMaxWidth())
        CommandComposer(identity, repository, enabled, direct, {
            direct = it
            if (it) keyboardRequest++
        }, { label ->
            val code = when (label) {
                "Ctrl+C" -> KeyEvent.KEYCODE_C
                "Esc" -> KeyEvent.KEYCODE_ESCAPE
                "Tab" -> KeyEvent.KEYCODE_TAB
                "←" -> KeyEvent.KEYCODE_DPAD_LEFT
                "→" -> KeyEvent.KEYCODE_DPAD_RIGHT
                "↑" -> KeyEvent.KEYCODE_DPAD_UP
                else -> KeyEvent.KEYCODE_DPAD_DOWN
            }
            input.key(code, if (label == "Ctrl+C") 2 else 0)
        }, onKeyboard = { keyboardRequest++ }, focused = focused, onToggleFocus = { focused = !focused },
            send = input::submit)
    }
}

@Composable
private fun TerminalHeader(title: String, endpoint: String, status: String, onBack: () -> Unit, onSessions: () -> Unit, onClose: (() -> Unit)? = null) {
    var menu by remember { mutableStateOf(false) }
    val connection = "$endpoint · ${statusLabel(status)}"
    Row(Modifier.fillMaxWidth().height(48.dp).background(MaterialTheme.colorScheme.background), verticalAlignment = Alignment.CenterVertically) {
        GlyphButton(R.drawable.ic_back, stringResource(R.string.back), onBack)
        Row(Modifier.weight(1f).heightIn(min = 48.dp)
            .clickable(onClickLabel = stringResource(R.string.switch_session), onClick = onSessions)
            .semantics { contentDescription = connection },
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(7.dp)) {
            if (status == "connecting") CircularProgressIndicator(Modifier.size(10.dp), strokeWidth = 1.5.dp)
            else Glyph(if (status == "ready") R.drawable.ic_terminal else R.drawable.ic_info, Modifier.size(15.dp),
                if (status == "failed") MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary)
            Text(title, fontSize = 13.sp, fontFamily = LocalTerminalFont.current, maxLines = 1,
                overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
            Glyph(R.drawable.ic_down, Modifier.size(12.dp))
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
fun DesktopScreen(desktop: DesktopWorkspace?, onPane: (DesktopPane) -> Unit, onRetry: (() -> Unit)? = null, onDisconnect: () -> Unit) {
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
        desktop?.failure?.let { HelperText(desktopFailureText(it), Modifier.padding(vertical = 12.dp)) }
        if (desktop?.status !in listOf("ready", "connecting")) HelperText(stringResource(R.string.device_unavailable), Modifier.padding(vertical = 12.dp))
        if (onRetry != null && desktop?.status !in listOf("ready", "connecting")) {
            Button(onRetry, modifier = Modifier.padding(top = 12.dp)) { Text(stringResource(R.string.retry)) }
        }
        OutlinedButton(onDisconnect, modifier = Modifier.padding(top = 20.dp)) { Text(stringResource(R.string.disconnect)) }
    }
}
