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
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
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
    val trust by repository.trust.collectAsStateWithLifecycle()
    var selectedFor by remember { mutableStateOf<String?>(null) }
    var waitingFile by remember(session.id) { mutableStateOf<Uri?>(null) }
    var password by remember(session.id) { mutableStateOf("") }
    var busy by remember(session.id) { mutableStateOf(false) }
    var job by remember(session.id) { mutableStateOf<Job?>(null) }
    var owner by remember(session.id) { mutableStateOf<String?>(null) }
    var failure by remember(session.id) { mutableStateOf<Int?>(null) }
    var uninsertedPath by remember(session.id) { mutableStateOf<String?>(null) }
    val currentId by rememberUpdatedState(session.id)
    val compose by rememberUpdatedState(onCompose)
    DisposableEffect(session.id) {
        onDispose { job?.cancel(); owner?.let(repository::endSshOperation) }
    }

    fun start(uri: Uri, entered: CharArray? = null) {
        if (busy || session.status != "ready") { entered?.fill('\u0000'); return }
        val sessionId = session.id
        val originalHost = session.host
        val host = originalHost?.let { old -> repository.hosts.value.firstOrNull {
            it.id == old.id && it.port == old.port && runCatching {
                parseSshEndpoint(it.address, it.user) == parseSshEndpoint(old.address, old.user)
            }.getOrDefault(false)
        } ?: old }
        busy = true
        failure = null
        job = scope.launch {
            var secret = entered
            var operation: String? = null
            try {
                if (host != null && secret == null) secret = repository.loadSavedPassword(host)
                if (host != null && secret == null) { waitingFile = uri; return@launch }
                val path = if (host == null) TerminalAttachments.importLocal(context, uri) else {
                    operation = repository.beginSshOperation()
                    owner = operation
                    TerminalAttachments.uploadSsh(context, uri, host, checkNotNull(secret)) { server, fingerprint ->
                        repository.verifySshOperation(checkNotNull(operation), server, fingerprint)
                    }
                }
                ensureActive()
                if (currentId != sessionId || repository.sessions.value.none { it.id == sessionId && it.status == "ready" }) return@launch
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
                secret?.fill('\u0000')
                operation?.let(repository::endSshOperation)
                owner = null
                busy = false
            }
        }
    }

    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        val belongsTo = selectedFor
        selectedFor = null
        if (uri != null && belongsTo == session.id) start(uri)
    }
    if (waitingFile != null) AlertDialog(onDismissRequest = { waitingFile = null; password = "" },
        title = { Text(stringResource(R.string.attachment_auth_title)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                HelperText(stringResource(R.string.attachment_auth_hint))
                ConnectionField(password, { password = it }, R.string.password, keyboard = KeyboardType.Password,
                    transformation = PasswordVisualTransformation(), limit = 1024)
            }
        }, confirmButton = {
            TextButton({
                val uri = waitingFile
                val secret = password.toCharArray()
                password = ""; waitingFile = null
                if (uri != null) start(uri, secret) else secret.fill('\u0000')
            }, enabled = password.isNotEmpty()) { Text(stringResource(R.string.attachment_upload)) }
        }, dismissButton = { TextButton({ waitingFile = null; password = "" }) { Text(stringResource(R.string.cancel)) } })
    if (busy && (owner == null || trust?.ownerId != owner)) AlertDialog(onDismissRequest = { job?.cancel() },
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
            selectedFor = session.id
            runCatching { picker.launch(arrayOf("*/*")) }.onFailure { selectedFor = null; failure = R.string.attachment_unreadable }
        }
    }, busy = busy)
}

private fun attachmentErrorText(error: Exception): Int = when (error) {
    is AttachmentException -> error.sshFailure?.let(::attachmentSshErrorText) ?: when (error.code) {
        AttachmentError.TOO_LARGE -> R.string.attachment_too_large
        AttachmentError.UNREADABLE -> R.string.attachment_unreadable
        AttachmentError.STORAGE -> R.string.attachment_storage
        AttachmentError.REMOTE_TOOLS -> R.string.attachment_remote_tools
        else -> R.string.attachment_transfer_failed
    }
    else -> attachmentSshErrorText(classifySshFailure(error))
}

private fun attachmentSshErrorText(kind: SshFailureKind): Int = when (kind) {
    SshFailureKind.AUTH -> R.string.ssh_error_auth
    SshFailureKind.HOST_KEY_CHANGED -> R.string.ssh_error_host_key
    SshFailureKind.TRUST_REJECTED -> R.string.ssh_error_trust
    SshFailureKind.TIMEOUT -> R.string.ssh_error_timeout
    else -> R.string.attachment_transfer_failed
}
