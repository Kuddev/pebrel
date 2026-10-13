package io.github.kuddev.pebrel.mobile.ui

import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.*
import io.github.kuddev.pebrel.mobile.session.DesktopWorkspace
import io.github.kuddev.pebrel.mobile.session.SessionRepository
import kotlinx.coroutines.*

@Composable
fun ConversationScreen(desktop: DesktopWorkspace, pane: DesktopPane, identity: ConversationIdentity,
                       repository: SessionRepository, onTerminal: () -> Unit, onTabs: () -> Unit, onFile: (DesktopTab) -> Unit) {
    val client = remember(desktop.id, desktop.connectionGeneration, desktop.runtimeProcess, pane.window, pane.id, identity) {
        repository.desktopConversation(desktop.id, pane, identity)
    }
    val page by client.state.collectAsStateWithLifecycle()
    val lifecycle = LocalLifecycleOwner.current
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    var failure by remember(client) { mutableStateOf<String?>(null) }
    var loading by remember(client) { mutableStateOf(!page.loaded) }
    var choosing by remember(client) { mutableStateOf(false) }
    var refresh by remember(client) { mutableIntStateOf(0) }
    val clipboard = rememberReaderClipboard(pane.displayTitle) { failure = "clipboard_failed" }
    val allowed = desktop.status == "ready" && desktop.allowInput
    val copyLabel = stringResource(R.string.reader_copy)
    val copiedLabel = stringResource(R.string.reader_copied)
    val imageLabel = stringResource(R.string.reader_open_image)
    val userLabel = stringResource(R.string.chat_you)
    val toolLabel = stringResource(R.string.chat_tool)
    val pendingLabel = stringResource(R.string.chat_tool_pending)
    val clippedLabel = stringResource(R.string.chat_content_clipped)
    val agentName = pane.agent?.name?.ifBlank { identity.kind } ?: identity.kind
    val colors = MaterialTheme.colorScheme.readerColors()
    val document by produceState<ReaderDocument?>(null, page.messages, colors, agentName, userLabel, toolLabel, copyLabel) {
        value = withContext(Dispatchers.Default) {
            prepareConversation(page.messages, colors, agentName, userLabel, toolLabel, pendingLabel, clippedLabel, copyLabel, imageLabel)
        }
    }
    LaunchedEffect(client, desktop.status, refresh) {
        if (desktop.status != "ready") return@LaunchedEffect
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (isActive) {
                try { client.refresh(); failure = null }
                catch (cancelled: CancellationException) { throw cancelled }
                catch (error: Exception) {
                    failure = conversationFailureCode(error)
                    if (failure in setOf("conversation_identity_changed", "conversation_ended", "desktop_session_changed", "method_not_found")) break
                } finally { loading = false }
                delay(if (client.state.value.state == "running") 1500 else 3000)
            }
        }
    }
    fun openLink(link: String) {
        val uri = Uri.parse(link)
        if (uri.scheme in setOf("https", "http", "mailto")) {
            try { context.startActivity(Intent(Intent.ACTION_VIEW, uri)) }
            catch (_: Exception) { failure = "link_open_failed" }
            return
        }
        val base = DesktopFile("${page.cwd.replace('\\', '/').trimEnd('/')}/conversation.md", false, false, false, true, null)
        val path = resolveReaderLink(base, link)
        if (path == null) { failure = "link_open_failed"; return }
        scope.launch {
            try { onFile(repository.desktopTabs(desktop.id).openFile(pane.window, path)) }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (error: Exception) { failure = conversationFailureCode(error) }
        }
    }
    Column(Modifier.fillMaxSize()) {
        TerminalHeader(pane.displayTitle, desktop.host.name, desktop.status, onTerminal, onTabs,
            onConversation = onTerminal, conversationActive = true)
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            HelperText("$agentName · ${if (desktop.status == "ready") statusLabel(page.state) else statusLabel(desktop.status)}", Modifier.weight(1f))
            if (page.before != null) TextButton({
                if (loading) return@TextButton
                loading = true
                scope.launch {
                    try { client.refresh(older = true); failure = null }
                    catch (cancelled: CancellationException) { throw cancelled }
                    catch (error: Exception) { failure = conversationFailureCode(error) }
                    finally { loading = false }
                }
            }, enabled = !loading && desktop.status == "ready") { Text(stringResource(R.string.chat_older)) }
        }
        if (loading) LinearProgressIndicator(Modifier.fillMaxWidth())
        if (desktop.status != "ready") HelperText(stringResource(R.string.reader_offline), Modifier.padding(horizontal = 16.dp, vertical = 6.dp))
        else if (!desktop.allowInput) HelperText(stringResource(R.string.composer_pc_read_only_short), Modifier.padding(horizontal = 16.dp, vertical = 6.dp))
        failure?.let { code -> Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(conversationFailureText(code), Modifier.weight(1f), color = MaterialTheme.colorScheme.error, fontSize = 13.sp)
            val returnToTerminal = code in setOf("conversation_identity_changed", "conversation_ended", "desktop_session_changed", "method_not_found")
            TextButton({ if (returnToTerminal) onTerminal() else refresh++ }) {
                Text(stringResource(if (returnToTerminal) R.string.chat_terminal else R.string.chat_refresh))
            }
        } }
        Box(Modifier.weight(1f).fillMaxWidth()) {
            document?.let { prepared ->
                ReaderWebView(prepared, colors, clipboard.copied, copyLabel, copiedLabel, null,
                    { index -> clipboard.copy(prepared.code[index], index) }, ::openLink, Modifier.fillMaxSize(), followUpdates = true)
            }
            if (page.loaded && page.messages.isEmpty()) HelperText(stringResource(R.string.chat_empty), Modifier.align(Alignment.Center).padding(24.dp))
        }
        page.prompt?.let { prompt -> ConversationChoices(prompt, allowed && !choosing) { index ->
            choosing = true
            scope.launch {
                try { client.choose(prompt, index); failure = null; refresh++ }
                catch (cancelled: CancellationException) { throw cancelled }
                catch (error: Exception) { failure = conversationFailureCode(error); refresh++ }
                finally { choosing = false }
            }
        } }
        if (page.prompt == null && page.state in setOf("waiting_input", "attention")) {
            TextButton(onTerminal, Modifier.fillMaxWidth()) { Text(stringResource(R.string.chat_terminal_question)) }
        }
        CommandComposer("${desktop.id}:${pane.window}:${pane.id}:chat:${identity.session}", repository,
            allowed && page.canSend && !choosing, false, null, { label ->
                scope.launch {
                    try { client.key(label); failure = null; refresh++ }
                    catch (cancelled: CancellationException) { throw cancelled }
                    catch (error: Exception) { failure = conversationFailureCode(error); refresh++ }
                }
            }, shortcutsEnabled = allowed && page.identity.epoch != null && !choosing, send = { text ->
                try { client.send(text); failure = null; refresh++; true }
                catch (cancelled: CancellationException) { throw cancelled }
                catch (error: Exception) {
                    failure = if (error is DesktopRpcFailure) error.code else "delivery_unknown"
                    false
                }
            })
    }
}

@Composable
internal fun ConversationChoices(prompt: ConversationPrompt, enabled: Boolean, onChoose: (Int) -> Unit) {
    Column(Modifier.fillMaxWidth().heightIn(max = 320.dp).verticalScroll(rememberScrollState()).padding(horizontal = 16.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp)) {
        HorizontalDivider()
        Text(prompt.text, Modifier.padding(vertical = 8.dp), fontSize = 14.sp)
        val labels = if (prompt.binary) listOf(stringResource(R.string.chat_allow), stringResource(R.string.chat_deny)) else prompt.options
        labels.forEachIndexed { index, label ->
            OutlinedButton({ onChoose(index) }, Modifier.fillMaxWidth().heightIn(min = 48.dp), enabled = enabled, shape = RoundedCornerShape(6.dp)) {
                Text(label, Modifier.fillMaxWidth())
            }
        }
    }
}

private fun conversationFailureCode(error: Exception): String = when (error) {
    is DesktopRpcFailure -> error.code
    is DesktopConnectionFailure -> "desktop_disconnected"
    else -> "conversation_unavailable"
}

@Composable
private fun conversationFailureText(code: String): String = when (code) {
    "method_not_found" -> stringResource(R.string.chat_update_desktop)
    "conversation_identity_changed", "conversation_ended", "desktop_session_changed" -> stringResource(R.string.chat_replaced)
    "conversation_unavailable" -> stringResource(R.string.chat_unavailable)
    "conversation_changed" -> stringResource(R.string.chat_refreshing)
    "conversation_history_limit" -> stringResource(R.string.chat_history_limit)
    "prompt_changed" -> stringResource(R.string.chat_prompt_changed)
    "agent_busy", "input_in_progress" -> stringResource(R.string.chat_busy)
    "delivery_unknown" -> stringResource(R.string.chat_delivery_unknown)
    else -> tabFailureText(code)
}
