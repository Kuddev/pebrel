package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.runtime.*
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.sshAgentName
import io.github.kuddev.pebrel.mobile.session.LocalSession
import io.github.kuddev.pebrel.mobile.session.SessionRepository
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive

/** Polls which Agent, if any, runs in the foreground of this SSH tab, while the tab is visible. */
@Composable
internal fun rememberSshAgent(session: LocalSession, repository: SessionRepository, enabled: Boolean): String? {
    var agent by remember(session.id, session.terminal) { mutableStateOf<String?>(null) }
    val lifecycle = LocalLifecycleOwner.current
    LaunchedEffect(session.id, session.terminal, session.status, enabled) {
        if (!enabled || session.host == null || session.status != "ready") { agent = null; return@LaunchedEffect }
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (isActive) {
                agent = try { repository.sshAgent(session.id) }
                    catch (cancelled: CancellationException) { throw cancelled }
                    catch (_: Exception) { null }
                delay(if (agent == null) 4000 else 8000)
            }
        }
    }
    return agent
}

/** The chat view of an Agent running in this app's own SSH terminal. */
@Composable
fun SshConversationScreen(session: LocalSession, kind: String, repository: SessionRepository, onTerminal: () -> Unit,
                          onSessions: () -> Unit) {
    val client = remember(session.id, session.terminal, kind) { repository.sshConversation(session.id, kind) }
    if (client == null) {
        LaunchedEffect(Unit) { onTerminal() }
        return
    }
    val attachments = rememberTerminalAttachmentAction(session, repository) {}
    val ready = session.status == "ready"
    val host = ConversationHost(
        title = session.title, agentName = sshAgentName(kind), ready = ready, allowInput = true,
        signal = session.status,
        // Shared with the terminal composer, so an uploaded attachment's path lands in this message.
        draftKey = session.id,
        notice = if (ready) null else R.string.reader_offline,
        attach = attachments,
    )
    ConversationView(client, host, repository, onTerminal) { onImmersive ->
        TerminalHeader(session.title, session.source, session.status, onTerminal, onSessions,
            conversationActive = true, standalone = false, onImmersive = onImmersive)
    }
}
