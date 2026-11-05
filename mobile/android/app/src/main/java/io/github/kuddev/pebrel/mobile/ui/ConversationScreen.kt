package io.github.kuddev.pebrel.mobile.ui

import android.content.Intent
import android.net.Uri
import android.os.SystemClock
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalTextToolbar
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.platform.UriHandler
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.*
import io.github.kuddev.pebrel.mobile.session.DesktopWorkspace
import io.github.kuddev.pebrel.mobile.session.SessionRepository
import kotlinx.coroutines.*
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.collectLatest

private val replacedCodes = setOf("conversation_identity_changed", "conversation_ended", "desktop_session_changed", "method_not_found")

@Composable
fun ConversationScreen(desktop: DesktopWorkspace, pane: DesktopPane, identity: ConversationIdentity,
                       repository: SessionRepository, onTerminal: () -> Unit, onTabs: () -> Unit, onFile: (DesktopTab) -> Unit) {
    val client = remember(desktop.id, desktop.connectionGeneration, desktop.runtimeProcess, pane.window, pane.id, identity) {
        repository.desktopConversation(desktop.id, pane, identity)
    }
    val host = ConversationHost(
        title = pane.displayTitle, agentName = pane.agent?.name?.ifBlank { identity.kind } ?: identity.kind,
        ready = desktop.status == "ready", allowInput = desktop.allowInput, signal = "${pane.state}:${pane.sequence}",
        draftKey = "${desktop.id}:${pane.window}:${pane.id}:chat:${identity.session}",
        notice = when {
            desktop.status != "ready" -> R.string.reader_offline
            !desktop.allowInput -> R.string.composer_pc_read_only_short
            else -> null
        },
        openFile = { path -> onFile(repository.desktopTabs(desktop.id).openFile(pane.window, path)) },
    )
    ConversationView(client, host, repository, onTerminal) { onImmersive ->
        TerminalHeader(pane.displayTitle, "${desktop.host.name} · ${desktop.transport}", desktop.status,
            onTerminal, onTabs, conversationActive = true, standalone = false, onImmersive = onImmersive)
    }
}

/** What a conversation view needs from the connection that owns the Agent. */
internal class ConversationHost(
    val title: String,
    val agentName: String,
    /** Reads run only while the transport is up. */
    val ready: Boolean,
    val allowInput: Boolean,
    /** Changes when the owner observes new Agent activity, so the reader wakes at once. */
    val signal: String,
    val draftKey: String,
    @androidx.annotation.StringRes val notice: Int? = null,
    /** Opens a file the transcript links to; null when the connection cannot open files. */
    val openFile: (suspend (String) -> Unit)? = null,
    val attach: TerminalAttachmentAction? = null,
)

@Composable
internal fun ConversationView(client: ConversationSource, host: ConversationHost, repository: SessionRepository,
                              onTerminal: () -> Unit, header: @Composable (onImmersive: () -> Unit) -> Unit) {
    val page by client.state.collectAsStateWithLifecycle()
    val lifecycle = LocalLifecycleOwner.current
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    var failure by remember(client) { mutableStateOf<String?>(null) }
    var loading by remember(client) { mutableStateOf(!page.loaded) }
    var choosing by remember(client) { mutableStateOf(false) }
    var refresh by remember(client) { mutableIntStateOf(0) }
    var reader by remember(client) { mutableStateOf<String?>(null) }
    val wake = remember(client) { Channel<Unit>(Channel.CONFLATED) }
    val clipboard = rememberReaderClipboard(host.title) { failure = "clipboard_failed" }
    val allowed = host.ready && host.allowInput
    val agentName = host.agentName
    var immersive by rememberSaveable(host.draftKey) { mutableStateOf(false) }
    ImmersiveSystemBars(immersive)

    val colors = MaterialTheme.colorScheme
    val terminalFont = LocalTerminalFont.current
    val imageLabel = stringResource(R.string.reader_open_image)
    val style = remember(colors.primary, colors.onSurface, terminalFont, imageLabel) {
        ChatMarkdownStyle(colors.primary, colors.onSurface.copy(alpha = .08f), terminalFont, imageLabel)
    }
    val builder = remember(style) { ConversationTimelineBuilder(style) }
    // Parsing runs off the main thread; unchanged messages reuse their parsed documents.
    val items by produceState(emptyList<ChatItem>(), page.messages, page.state, builder) {
        value = withContext(Dispatchers.Default) { builder.build(page.messages, page.state) }
    }
    val working = showsWorking(items, page.state)

    // Snapshot pushes carry task-state changes; read at once instead of waiting for the next poll.
    val signal = host.signal
    var seenSignal by remember(client) { mutableStateOf(signal) }
    LaunchedEffect(signal) {
        if (signal != seenSignal) {
            seenSignal = signal
            wake.trySend(Unit)
        }
    }
    LaunchedEffect(client, host.ready, refresh) {
        if (!host.ready) return@LaunchedEffect
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (isActive) {
                var changed = false
                try { changed = client.refresh(); failure = null }
                catch (cancelled: CancellationException) { throw cancelled }
                catch (error: Exception) {
                    failure = conversationFailureCode(error)
                    if (failure in replacedCodes) break
                } finally { loading = false }
                withTimeoutOrNull(conversationPollDelay(client.state.value.state, changed)) { wake.receive() }
            }
        }
    }
    // Elapsed time is shown only for a run this view saw begin; a run already under way has no known start.
    var runningSince by remember(client) { mutableStateOf<Long?>(null) }
    var previousState by remember(client) { mutableStateOf<String?>(null) }
    LaunchedEffect(page.state, page.loaded) {
        if (!page.loaded) return@LaunchedEffect
        if (page.state == "running" && previousState != null && previousState != "running") runningSince = SystemClock.elapsedRealtime()
        if (page.state != "running") runningSince = null
        previousState = page.state
    }
    var now by remember { mutableLongStateOf(SystemClock.elapsedRealtime()) }
    LaunchedEffect(runningSince) {
        while (runningSince != null) {
            now = SystemClock.elapsedRealtime()
            delay(1000)
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
        val open = host.openFile
        if (path == null || open == null) { failure = "link_open_failed"; return }
        scope.launch {
            try { open(path) }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (error: Exception) { failure = conversationFailureCode(error) }
        }
    }
    fun mutate(action: suspend () -> Unit) {
        scope.launch {
            try { action(); failure = null }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (error: Exception) { failure = conversationFailureCode(error) }
            finally { wake.trySend(Unit) }
        }
    }

    val listState = rememberLazyListState()
    val platformToolbar = LocalTextToolbar.current
    val toolbar = remember(platformToolbar) { ObservingTextToolbar(platformToolbar) }
    var follow by remember(client) { mutableStateOf(true) }
    var unseen by remember(client) { mutableStateOf(false) }
    // Only a finished scroll changes whether the list follows; growth below the viewport does not.
    LaunchedEffect(listState) {
        snapshotFlow { listState.isScrollInProgress }.collect { scrolling ->
            if (!scrolling) {
                follow = !listState.canScrollForward
                if (follow) unseen = false
            }
        }
    }
    val linkTarget = rememberUpdatedState<(String) -> Unit> { openLink(it) }
    val terminalTarget = rememberUpdatedState(onTerminal)
    val uriHandler = remember { object : UriHandler { override fun openUri(uri: String) = linkTarget.value(uri) } }
    val actions = remember(clipboard) { ChatActions(clipboard, onReader = { reader = it }, onTerminal = { terminalTarget.value() }) }
    val tail = Triple(items.size, items.lastOrNull(), working)

    GlassPage(
        topBar = {
            // Immersive reading keeps only a way back; the system bars are hidden too.
            if (immersive) Row(Modifier.fillMaxWidth().padding(8.dp), horizontalArrangement = Arrangement.End) {
                GlyphButton(R.drawable.ic_focus, stringResource(R.string.chat_exit_immersive), { immersive = false },
                    modifier = Modifier.glass(CircleShape, GlassTier.Light, GlassLift.Raised))
            } else Column(Modifier.fillMaxWidth().glassBar()) {
                header { immersive = true }
                SessionModeTabs(conversation = true, onSwitch = onTerminal)
                if (loading) LinearProgressIndicator(Modifier.fillMaxWidth().height(2.dp)) else Spacer(Modifier.height(2.dp))
            }
        },
        bottomBar = { Column(Modifier.fillMaxWidth()) {
            Column(Modifier.fillMaxWidth().padding(horizontal = 10.dp).padding(top = 8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp)) {
                if (!follow) BackToLatest(unseen, {
                    follow = true
                    unseen = false
                    scope.launch { listState.scrollToEnd() }
                }, Modifier.align(Alignment.End))
                host.notice?.let { ConversationNotice(stringResource(it)) }
                failure?.let { code ->
                    val replaced = code in replacedCodes
                    ConversationNotice(conversationFailureText(code), error = true,
                        action = stringResource(if (replaced) R.string.chat_terminal else R.string.chat_refresh)) {
                        if (replaced) onTerminal() else refresh++
                    }
                }
                page.prompt?.let { prompt ->
                    key(prompt.id) {
                        PromptCard(prompt, allowed && !choosing) { index ->
                            choosing = true
                            scope.launch {
                                try { client.choose(prompt, index); failure = null }
                                catch (cancelled: CancellationException) { throw cancelled }
                                catch (error: Exception) { failure = conversationFailureCode(error) }
                                finally { choosing = false; wake.trySend(Unit) }
                            }
                        }
                    }
                }
                if (page.prompt == null && page.state in setOf("waiting_input", "attention")) {
                    ConversationNotice(stringResource(R.string.chat_terminal_question), action = stringResource(R.string.chat_terminal),
                        onAction = onTerminal)
                }
                if (page.state in setOf("running", "waiting_input", "attention", "failed")) {
                    ConversationStatusLine(page.state, "$agentName · ${statusLabel(page.state)}",
                        elapsedLabelSeconds(runningSince, now),
                        canInterrupt = page.state == "running" && allowed && page.identity.epoch != null,
                        onInterrupt = { mutate { client.key("Esc") } })
                }
            }
            CommandComposer(host.draftKey, repository,
                allowed && page.canSend && !choosing, false, null, { label -> mutate { client.key(label) } },
                shortcutsEnabled = allowed && page.identity.epoch != null && !choosing, conversation = true,
                onAttach = host.attach?.pick, attachmentBusy = host.attach?.busy == true, send = { text ->
                    try { client.send(text); failure = null; true }
                    catch (cancelled: CancellationException) { throw cancelled }
                    catch (error: Exception) {
                        failure = if (error is DesktopRpcFailure) error.code else "delivery_unknown"
                        false
                    } finally { wake.trySend(Unit) }
                })
        } },
    ) { padding, source ->
        val contentPadding = PaddingValues(start = 16.dp, end = 16.dp,
            top = padding.calculateTopPadding() + 12.dp, bottom = padding.calculateBottomPadding() + 12.dp)
        // Follow new output, a growing composer or a resized viewport, unless the user scrolled away or is selecting.
        val following = follow && !toolbar.selecting
        LaunchedEffect(listState, tail) {
            if (items.isEmpty()) return@LaunchedEffect
            if (!follow) unseen = true
            else if (following && !listState.isScrollInProgress) listState.scrollToEnd()
        }
        LaunchedEffect(listState, padding.calculateBottomPadding()) {
            snapshotFlow { listState.layoutInfo.viewportSize.height }.collectLatest {
                if (follow && !toolbar.selecting && !listState.isScrollInProgress) listState.scrollToEnd()
            }
        }
        CompositionLocalProvider(LocalUriHandler provides uriHandler, LocalTextToolbar provides toolbar) {
            Box(Modifier.fillMaxSize()) {
                ChatTimeline(items, working, agentName, listState, contentPadding,
                    hasOlder = page.before != null, olderEnabled = !loading && host.ready, onOlder = {
                        if (!loading) {
                            loading = true
                            scope.launch {
                                try { client.refresh(older = true); failure = null }
                                catch (cancelled: CancellationException) { throw cancelled }
                                catch (error: Exception) { failure = conversationFailureCode(error) }
                                finally { loading = false }
                            }
                        }
                    }, actions = actions, modifier = Modifier.fillMaxSize().then(source))
                if (page.loaded && page.messages.isEmpty()) HelperText(stringResource(R.string.chat_empty),
                    Modifier.align(Alignment.Center).padding(24.dp))
            }
        }
    }
    reader?.let { source -> MessageReaderSheet(source, host.title, clipboard, ::openLink) { reader = null } }
}

/** The rich reader typesets formulas and diagrams the native timeline shows as source. */
@Composable
private fun MessageReaderSheet(source: String, title: String, clipboard: ReaderClipboard, onLink: (String) -> Unit,
                               onDismiss: () -> Unit) {
    val colors = MaterialTheme.colorScheme.readerColors()
    val copyLabel = stringResource(R.string.reader_copy)
    val copiedLabel = stringResource(R.string.reader_copied)
    val imageLabel = stringResource(R.string.reader_open_image)
    val document by produceState<ReaderDocument?>(null, source, colors) {
        value = withContext(Dispatchers.Default) { prepareReader(source, "$title.md", true, colors, copyLabel, imageLabel) }
    }
    val height = LocalConfiguration.current.screenHeightDp.dp * .72f
    GlassSheet(onDismiss) {
        Box(Modifier.fillMaxWidth().height(height)) {
            document?.let { prepared ->
                ReaderWebView(prepared, colors, clipboard.copied, copyLabel, copiedLabel, null,
                    { index -> clipboard.copy(prepared.code[index], index) }, onLink, Modifier.fillMaxSize())
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
    "python_missing" -> stringResource(R.string.chat_python_missing)
    "terminal_unknown" -> stringResource(R.string.chat_terminal_unknown)
    else -> tabFailureText(code)
}
