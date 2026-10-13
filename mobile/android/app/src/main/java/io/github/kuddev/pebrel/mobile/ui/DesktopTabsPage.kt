package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.*
import io.github.kuddev.pebrel.mobile.session.DesktopWorkspace
import io.github.kuddev.pebrel.mobile.session.SessionRepository
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

@Composable
fun DesktopTabsPage(desktop: DesktopWorkspace?, repository: SessionRepository, onBack: () -> Unit,
                    onTab: (DesktopTab) -> Unit, onPane: (DesktopPane) -> Unit,
                    onRetry: (() -> Unit)?, onDisconnect: () -> Unit) {
    val scope = rememberCoroutineScope()
    // 首次快照才带进程身份；连接中创建的句柄不能沿用到 ready 状态。
    val client = remember(desktop?.id, desktop?.connectionGeneration, desktop?.runtimeProcess) { desktop?.let { repository.desktopTabs(it.id) } }
    var addMenu by remember { mutableStateOf(false) }
    var adding by remember { mutableStateOf<String?>(null) }
    var closing by remember { mutableStateOf<DesktopTab?>(null) }
    var value by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<String?>(null) }
    var computerMenu by remember { mutableStateOf(false) }
    var changingAddress by remember { mutableStateOf<RelayProfile?>(null) }
    val savedComputer = desktop?.relayProfile?.takeIf { desktop.hasConnected && it.canRediscover }
    val active = desktop?.tabs?.find { it.active } ?: desktop?.tabs?.firstOrNull()
    val window = active?.window ?: desktop?.windows?.firstOrNull()
    val directory = active?.primaryPane?.takeIf { it.sshDestination == null }?.cwd
        ?: active?.file?.takeUnless { it.remote }?.path?.replace('\\', '/')?.substringBeforeLast('/')
    val enabled = desktop?.status == "ready" && desktop.allowInput && !busy
    fun begin(kind: String) {
        addMenu = false; failure = null
        value = directory.orEmpty().let { if (kind == "file" && it.isNotBlank()) it.trimEnd('/', '\\') + "/" else it }
        adding = kind
    }
    Column(Modifier.fillMaxSize()) {
        PageHeader(stringResource(R.string.computer_tabs), onBack) {
            Box {
                GlyphButton(R.drawable.ic_plus, stringResource(R.string.tab_new), { addMenu = true }, enabled)
                DropdownMenu(addMenu, { addMenu = false }) {
                    DropdownMenuItem({ Text(stringResource(R.string.tab_new_terminal)) }, { begin("terminal") })
                    DropdownMenuItem({ Text(stringResource(R.string.tab_open_file)) }, { begin("file") }, enabled = window != null)
                }
            }
            if (savedComputer != null) Box {
                GlyphButton(R.drawable.ic_more, stringResource(R.string.more_actions), { computerMenu = true })
                DropdownMenu(computerMenu, { computerMenu = false }) {
                    DropdownMenuItem({ Text(stringResource(R.string.computer_edit_address)) }, {
                        computerMenu = false
                        changingAddress = savedComputer
                    })
                }
            }
        }
        DesktopScreen(desktop, onPane, onRetry, onTab = onTab, onCloseTab = { closing = it }, onDisconnect = onDisconnect)
    }
    changingAddress?.let { profile ->
        ComputerAddressDialog(profile, { changingAddress = null }) { replacement ->
            changingAddress = null
            repository.connectRelay(replacement)
        }
    }
    adding?.let { kind ->
        AlertDialog(onDismissRequest = { if (!busy) adding = null },
            title = { Text(stringResource(if (kind == "terminal") R.string.tab_new_terminal else R.string.tab_open_file)) },
            text = { Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                OutlinedTextField(value, { value = it; failure = null }, Modifier.fillMaxWidth(), enabled = !busy,
                    label = { Text(stringResource(if (kind == "terminal") R.string.tab_directory else R.string.tab_file_path)) }, singleLine = true)
                if (kind == "file") HelperText(stringResource(R.string.tab_file_types))
                failure?.let { Text(tabFailureText(it), color = MaterialTheme.colorScheme.error) }
                if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
            } },
            confirmButton = { TextButton({
                if (busy || client == null) return@TextButton
                busy = true
                scope.launch {
                    try {
                        val tab = if (kind == "terminal") client.newTerminal(window, value.takeIf(String::isNotBlank))
                            else client.openFile(checkNotNull(window), value.trim())
                        adding = null; onTab(tab)
                    } catch (cancelled: CancellationException) { throw cancelled }
                    catch (error: Exception) { failure = tabFailureCode(error) }
                    finally { busy = false }
                }
            }, enabled = enabled && (kind == "terminal" || value.isNotBlank())) { Text(stringResource(R.string.tab_open)) } },
            dismissButton = { TextButton({ adding = null }, enabled = !busy) { Text(stringResource(R.string.cancel)) } })
    }
    closing?.let { tab -> if (client != null) DesktopTabCloseDialog(tab, client, { closing = null }, { closing = null }) }
}

@Composable
internal fun DesktopTabRows(desktop: DesktopWorkspace, onTab: (DesktopTab) -> Unit,
                            onPane: (DesktopPane) -> Unit, onClose: ((DesktopTab) -> Unit)?) {
    desktop.tabs.forEach { tab ->
        key(tab.key) {
            Row(Modifier.fillMaxWidth().heightIn(min = 64.dp), verticalAlignment = Alignment.CenterVertically) {
                Row(Modifier.weight(1f).heightIn(min = 64.dp)
                    .clickable(enabled = desktop.status == "ready" && (tab.readable || tab.panes.isNotEmpty())) { onTab(tab) }
                    .padding(vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.width(36.dp), contentAlignment = Alignment.CenterStart) { DesktopTabSymbol(tab) }
                    Column(Modifier.weight(1f)) {
                        Text(tab.displayTitle, fontSize = 14.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(tabKindLabel(tab), fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    if (tab.active) Glyph(R.drawable.ic_check, Modifier.padding(horizontal = 8.dp).size(16.dp), MaterialTheme.colorScheme.primary)
                }
                if (onClose != null && tab.id != null) GlyphButton(R.drawable.ic_close,
                    stringResource(R.string.tab_close_named, tab.displayTitle), { onClose(tab) }, desktop.status == "ready" && desktop.allowInput)
            }
            if (tab.panes.size > 1) tab.panes.forEach { pane ->
                TextButton({ onPane(pane) }, Modifier.fillMaxWidth().padding(start = 36.dp), enabled = desktop.status == "ready") {
                    Text("${pane.id} · ${pane.title.ifBlank { pane.displayTitle }}", Modifier.fillMaxWidth(), maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
        }
    }
}

@Composable
internal fun DesktopTabCloseDialog(tab: DesktopTab, client: DesktopTabs, onDismiss: () -> Unit, onClosed: () -> Unit) {
    val scope = rememberCoroutineScope()
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    AlertDialog(onDismissRequest = { if (!busy) onDismiss() }, title = { Text(stringResource(R.string.tab_close)) },
        text = { Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(stringResource(R.string.tab_close_confirm, tab.displayTitle))
            error?.let { Text(tabFailureText(it), color = MaterialTheme.colorScheme.error) }
            if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
        } },
        confirmButton = { TextButton({
            if (busy) return@TextButton
            busy = true
            scope.launch {
                try {
                    check(client.close(tab).getJSONObject("action").getBoolean("closed"))
                    onClosed()
                } catch (cancelled: CancellationException) { throw cancelled }
                catch (failure: Exception) { error = tabFailureCode(failure) }
                finally { busy = false }
            }
        }, enabled = !busy) { Text(stringResource(R.string.tab_close)) } },
        dismissButton = { TextButton(onDismiss, enabled = !busy) { Text(stringResource(R.string.cancel)) } })
}

internal fun tabIcon(tab: DesktopTab): Int = when (tab.kind) {
    "image" -> R.drawable.ic_image
    "document", "code" -> R.drawable.ic_git_file
    else -> R.drawable.ic_terminal
}

@Composable
internal fun DesktopTabSymbol(tab: DesktopTab, modifier: Modifier = Modifier) {
    if (tab.kind in setOf("document", "code", "image")) {
        FileSymbol(tab.file?.path ?: tab.displayTitle, modifier = modifier.size(20.dp))
    } else Glyph(tabIcon(tab), modifier)
}

@Composable
internal fun tabKindLabel(tab: DesktopTab): String = stringResource(when (tab.kind) {
    "image" -> R.string.tab_kind_image
    "document" -> R.string.tab_kind_document
    "code" -> R.string.tab_kind_code
    "ssh" -> R.string.tab_kind_ssh
    else -> R.string.tab_kind_terminal
})

internal fun tabFailureCode(error: Exception): String = when (error) {
    is DesktopRpcFailure -> error.code
    is DesktopConnectionFailure -> "desktop_disconnected"
    else -> "file_read_failed"
}

@Composable
internal fun tabFailureText(code: String): String = stringResource(when (code) {
    "tabs_unsupported", "method_not_found" -> R.string.tabs_unsupported
    "input_not_authorized" -> R.string.composer_pc_read_only
    "desktop_session_changed", "target_not_found" -> R.string.desktop_session_changed
    "desktop_disconnected" -> R.string.reader_disconnected
    "confirmation_required" -> R.string.tab_busy
    "unsaved_changes" -> R.string.tab_unsaved
    "file_saving", "close_pending" -> R.string.tab_saving
    "file_changed" -> R.string.reader_changed
    "file_too_large" -> R.string.reader_too_large
    "unsupported_file" -> R.string.reader_unsupported
    "image_decode_failed" -> R.string.reader_image_failed
    "link_open_failed" -> R.string.reader_link_failed
    "clipboard_failed" -> R.string.reader_copy_failed
    else -> R.string.reader_load_failed
})
