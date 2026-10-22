package io.github.kuddev.pebrel.mobile.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.net.Uri
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.*
import io.github.kuddev.pebrel.mobile.session.LocalSession
import io.github.kuddev.pebrel.mobile.session.SessionRepository
import kotlinx.coroutines.*

data class TerminalAttachmentAction(val pick: () -> Unit, val busy: Boolean)

/** File selection is owned by the visible terminal; it never sends a shell command. */
@Composable
fun rememberTerminalAttachmentAction(session: LocalSession, repository: SessionRepository,
                                     onCompose: () -> Unit): TerminalAttachmentAction {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var selectedFor by remember { mutableStateOf<Any?>(null) }
    var busy by remember(session.terminal) { mutableStateOf(false) }
    var job by remember(session.terminal) { mutableStateOf<Job?>(null) }
    var failure by remember(session.terminal) { mutableStateOf<Int?>(null) }
    var uninsertedPath by remember(session.terminal) { mutableStateOf<String?>(null) }
    val currentTerminal by rememberUpdatedState(session.terminal)
    val compose by rememberUpdatedState(onCompose)
    DisposableEffect(session.terminal) {
        onDispose { job?.cancel() }
    }

    fun start(uri: Uri) {
        if (busy || session.status != "ready") return
        val sessionId = session.id
        val terminal = session.terminal
        busy = true
        failure = null
        job = scope.launch {
            try {
                val path = if (session.host == null) TerminalAttachments.importLocal(context, uri)
                    else TerminalAttachments.uploadSsh(context, uri, checkNotNull(session.files))
                ensureActive()
                if (currentTerminal !== terminal || repository.sessions.value.none {
                    it.id == sessionId && it.terminal === terminal && it.status == "ready"
                }) return@launch
                val quoted = "'" + path.replace("'", "'\\''") + "'"
                val previous = repository.drafts.value[sessionId].orEmpty()
                val next = previous + (if (previous.isNotEmpty() && !previous.last().isWhitespace()) " " else "") + quoted + " "
                if (next.length > 8192) uninsertedPath = path
                else {
                    repository.setDraft(sessionId, next)
                    compose()
                    Toast.makeText(context, R.string.attachment_added, Toast.LENGTH_SHORT).show()
                }
            } catch (_: TimeoutCancellationException) { failure = R.string.ssh_error_timeout }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (error: Exception) { failure = attachmentErrorText(error) }
            finally {
                busy = false
            }
        }
    }

    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        val belongsTo = selectedFor
        selectedFor = null
        if (uri != null && belongsTo === session.terminal) start(uri)
    }
    if (busy) AlertDialog(onDismissRequest = { job?.cancel() },
        title = { Text(stringResource(if (session.host == null) R.string.attachment_importing else R.string.attachment_uploading)) },
        text = { LinearProgressIndicator(Modifier.fillMaxWidth()) },
        confirmButton = { TextButton({ job?.cancel() }) { Text(stringResource(R.string.cancel)) } })
    failure?.let { message -> AlertDialog(onDismissRequest = { failure = null },
        title = { Text(stringResource(R.string.attachment_failed)) }, text = { Text(stringResource(message)) },
        confirmButton = { TextButton({ failure = null }) { Text(stringResource(R.string.close)) } }) }
    uninsertedPath?.let { path -> AlertDialog(onDismissRequest = { uninsertedPath = null },
        title = { Text(stringResource(R.string.attachment_draft_full)) },
        text = { SelectionContainer { Text(path, fontFamily = LocalTerminalFont.current) } },
        confirmButton = { TextButton({
            context.getSystemService(ClipboardManager::class.java).setPrimaryClip(ClipData.newPlainText("Pebrel", path))
            uninsertedPath = null
        }) { Text(stringResource(R.string.attachment_copy_path)) } },
        dismissButton = { TextButton({ uninsertedPath = null }) { Text(stringResource(R.string.close)) } }) }
    return TerminalAttachmentAction(pick = {
        if (!busy && session.status == "ready") {
            selectedFor = session.terminal
            runCatching { picker.launch(arrayOf("*/*")) }.onFailure { selectedFor = null; failure = R.string.attachment_unreadable }
        }
    }, busy = busy)
}

private fun attachmentErrorText(error: Exception): Int = when (error) {
    is AttachmentException -> error.sshFailure?.let(::attachmentSshErrorText) ?: when (error.code) {
        AttachmentError.TOO_LARGE -> R.string.attachment_too_large
        AttachmentError.UNREADABLE -> R.string.attachment_unreadable
        AttachmentError.STORAGE -> R.string.attachment_storage
        else -> R.string.attachment_transfer_failed
    }
    else -> attachmentSshErrorText(classifySshFailure(error))
}

private fun attachmentSshErrorText(kind: SshFailureKind): Int = when (kind) {
    SshFailureKind.AUTH -> R.string.ssh_error_auth
    SshFailureKind.KEY -> R.string.ssh_error_key
    SshFailureKind.HOST_KEY_CHANGED -> R.string.ssh_error_host_key
    SshFailureKind.TRUST_REJECTED -> R.string.ssh_error_trust
    SshFailureKind.TIMEOUT -> R.string.ssh_error_timeout
    else -> R.string.attachment_transfer_failed
}
