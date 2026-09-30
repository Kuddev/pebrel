package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.foundation.layout.*
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.HostProfile
import io.github.kuddev.pebrel.mobile.connection.SshSessionMode
import io.github.kuddev.pebrel.mobile.connection.validRemoteSessionName
import io.github.kuddev.pebrel.mobile.connection.parseSshEndpoint
import io.github.kuddev.pebrel.mobile.connection.endpointLabel
import io.github.kuddev.pebrel.mobile.session.TrustRequest
import java.util.UUID

@Composable
fun HostForm(
    initial: HostProfile?,
    onCancel: () -> Unit,
    passwordSaved: Boolean,
    busy: Boolean,
    onClearPassword: () -> Unit,
    onSave: (HostProfile, CharArray?, Boolean, Boolean) -> Unit,
) {
    var name by rememberSaveable { mutableStateOf(initial?.name.orEmpty()) }
    var address by rememberSaveable { mutableStateOf(initial?.address.orEmpty()) }
    var user by rememberSaveable { mutableStateOf(initial?.user ?: "root") }
    var port by rememberSaveable { mutableStateOf((initial?.port ?: 22).toString()) }
    var icon by rememberSaveable { mutableStateOf(initial?.icon ?: "term") }
    var group by rememberSaveable { mutableStateOf(initial?.group ?: "development") }
    var sessionMode by rememberSaveable { mutableStateOf(initial?.sessionMode ?: SshSessionMode.SHELL) }
    var sessionName by rememberSaveable { mutableStateOf(initial?.sessionName.orEmpty()) }
    var password by remember { mutableStateOf("") }
    var auth by rememberSaveable { mutableStateOf(if (initial?.keyUri.isNullOrEmpty()) "auto" else "key") }
    var keyUri by rememberSaveable { mutableStateOf(initial?.keyUri.orEmpty()) }
    var keyName by rememberSaveable { mutableStateOf(initial?.keyName.orEmpty()) }
    var keyError by remember { mutableStateOf(false) }
    var picking by remember { mutableStateOf(false) }
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val chooseKey = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) scope.launch {
            picking = true
            try {
                val name = withContext(Dispatchers.IO) {
                    context.contentResolver.query(uri, arrayOf(android.provider.OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                        if (it.moveToFirst()) it.getString(0) else null
                    }.orEmpty()
                }
                keyUri = uri.toString(); keyName = name.take(160); keyError = false
            } catch (cancelled: kotlinx.coroutines.CancellationException) { throw cancelled }
            catch (_: Exception) { keyError = true }
            finally { picking = false }
        }
    }
    val endpoint = runCatching { parseSshEndpoint(address, user) }.getOrNull()
    val originalEndpoint = initial?.let { runCatching { parseSshEndpoint(it.address, it.user) }.getOrNull() }
    val passwordIsSaved = passwordSaved && initial != null && originalEndpoint == endpoint &&
        initial.port == port.toIntOrNull() && initial.keyUri == if (auth == "key") keyUri else ""
    var rememberPassword by rememberSaveable(initial?.id) { mutableStateOf(passwordSaved || initial == null) }
    val valid = name.isNotBlank() && endpoint != null && (port.toIntOrNull() ?: 0) in 1..65535 &&
        (sessionMode == SshSessionMode.SHELL || validRemoteSessionName(sessionMode, sessionName)) &&
        (auth != "key" || keyUri.isNotEmpty()) && !picking
    fun profile() = HostProfile(initial?.id ?: UUID.randomUUID().toString(), name.trim(), checkNotNull(endpoint).address, port.toInt(), endpoint.user,
        icon = icon, group = group, sessionMode = sessionMode, sessionName = sessionName,
        keyUri = if (auth == "key") keyUri else "", keyName = if (auth == "key") keyName else "")
    ConnectionForm(stringResource(if (initial == null) R.string.add_ssh else R.string.edit_host), { if (!busy) onCancel() }) {
        if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
        Column(Modifier.padding(top = 4.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                HostIconChoice(icon) { icon = it }
                ConnectionField(name, { name = it }, R.string.host_name, Modifier.weight(1f),
                    placeholder = stringResource(R.string.host_name), limit = 40, showLabel = false)
            }
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                ConnectionField(address, { value ->
                    address = value
                    if ('@' in value) runCatching { parseSshEndpoint(value, user) }.onSuccess {
                        address = it.address; user = it.user
                    }
                }, R.string.host_address, Modifier.weight(1f),
                    keyboard = KeyboardType.Uri, placeholder = "server.example.com")
                ConnectionField(port, { port = it }, R.string.port, Modifier.width(82.dp), KeyboardType.Number, limit = 5)
            }
            ConnectionField(user, { user = it }, R.string.username, limit = 40)
            SegmentRow(R.string.ssh_terminal_mode) {
                ConnectionSegments(SshSessionMode.entries.map { it.id to if (it == SshSessionMode.SHELL) "Shell" else it.id },
                    sessionMode.id, { id -> sessionMode = SshSessionMode.entries.first { it.id == id } }, Modifier.testTag("ssh-session-mode"), compact = true)
            }
            if (sessionMode != SshSessionMode.SHELL) {
                ConnectionField(sessionName, { sessionName = it }, R.string.ssh_persistent_session, limit = 64)
                HelperText(stringResource(R.string.ssh_multiplexer_hint))
                if (!validRemoteSessionName(sessionMode, sessionName)) Text(stringResource(R.string.ssh_session_name_invalid),
                    color = MaterialTheme.colorScheme.error, fontSize = 12.sp)
            }
            SegmentRow(R.string.authentication) {
                ConnectionSegments(listOf("auto" to stringResource(R.string.auth_auto), "key" to stringResource(R.string.auth_key)),
                    auth, { if (!busy && auth != it) { password = ""; auth = it } }, Modifier.testTag("ssh-auth-mode"), compact = true)
            }
            if (auth == "key") {
                OutlinedButton({ chooseKey.launch(arrayOf("*/*")) }, enabled = !busy && !picking, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)) {
                    Text(keyName.ifBlank { stringResource(R.string.ssh_choose_key) }, maxLines = 2)
                }
                if (picking) LinearProgressIndicator(Modifier.fillMaxWidth())
                HelperText(stringResource(R.string.ssh_key_file_hint))
                if (keyError) Text(stringResource(R.string.ssh_key_access_failed), color = MaterialTheme.colorScheme.error)
            }
            ConnectionField(password, { password = it }, if (auth == "key") R.string.ssh_key_passphrase else R.string.credential_password,
                keyboard = KeyboardType.Password, placeholder = if (passwordIsSaved) stringResource(R.string.password_saved_placeholder) else "",
                transformation = PasswordVisualTransformation(), limit = 1024)
            HelperText(stringResource(if (auth == "key") R.string.ssh_key_passphrase_hint else R.string.ssh_password_optional_hint))
            Column {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(rememberPassword, { rememberPassword = it }, enabled = !busy)
                    Text(stringResource(if (auth == "key") R.string.ssh_save_passphrase else R.string.save_password), fontSize = 13.sp, modifier = Modifier.weight(1f))
                    if (passwordIsSaved) {
                        TextButton({
                            password = ""
                            onClearPassword()
                        }, enabled = !busy) { Text(stringResource(if (auth == "key") R.string.ssh_clear_saved_passphrase else R.string.clear_saved_password), fontSize = 12.sp) }
                    }
                }
                HelperText(stringResource(
                    when {
                        auth == "key" && passwordIsSaved && rememberPassword -> R.string.ssh_passphrase_saved_hint
                        auth == "key" && rememberPassword -> R.string.ssh_passphrase_will_save_hint
                        auth == "key" -> R.string.ssh_passphrase_not_saved_hint
                        passwordIsSaved && rememberPassword -> R.string.password_saved_hint
                        rememberPassword -> R.string.password_will_save_hint
                        else -> R.string.password_not_saved_hint
                    },
                ), Modifier.padding(start = 12.dp))
            }
            SegmentRow(R.string.host_group) {
                ConnectionSegments(listOf("production" to stringResource(R.string.group_production), "development" to stringResource(R.string.group_development)),
                    group, { group = it }, Modifier.testTag("ssh-host-group"), compact = true)
            }
        }
        Spacer(Modifier.height(24.dp))
        fun submit(connect: Boolean) {
            val secret = password.takeIf { it.isNotEmpty() }?.toCharArray()
            onSave(profile(), secret, rememberPassword, connect)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            OutlinedButton({ submit(false) }, enabled = valid && !busy,
                modifier = Modifier.weight(1f).heightIn(min = 48.dp), shape = MaterialTheme.shapes.medium) {
                Text(stringResource(R.string.save))
            }
            Button({ submit(true) }, enabled = valid && !busy,
                modifier = Modifier.weight(1.6f).heightIn(min = 48.dp), shape = MaterialTheme.shapes.medium) {
                Text(stringResource(R.string.save_connect))
            }
        }
    }
}

@Composable
private fun SegmentRow(label: Int, content: @Composable RowScope.() -> Unit) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(stringResource(label), fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.widthIn(min = 76.dp, max = 106.dp))
        content()
    }
}

@Composable
fun LoginForm(host: HostProfile, onCancel: () -> Unit, passwordSaved: Boolean, busy: Boolean,
              onClearPassword: () -> Unit, attachmentLabel: String? = null,
              onConnect: (CharArray?, Boolean, Boolean, Boolean) -> Unit) {
    var password by remember { mutableStateOf("") }
    var rememberPassword by rememberSaveable(host.id) { mutableStateOf(true) }
    var desktop by remember { mutableStateOf(false) }
    var input by remember { mutableStateOf(false) }
    ConnectionForm(stringResource(R.string.connect), { if (!busy) onCancel() }) {
        if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(13.dp)) {
            HostSymbol(host.icon, Modifier.size(30.dp))
            Column {
                Text(host.name, fontSize = 16.sp)
                HelperText(host.endpointLabel, Modifier.padding(top = 6.dp))
            }
        }
        Spacer(Modifier.height(24.dp))
        if (host.keyUri.isNotEmpty()) HelperText(host.keyName.ifBlank { stringResource(R.string.auth_key) })
        ConnectionField(password, { password = it }, if (host.keyUri.isNotEmpty()) R.string.ssh_key_passphrase else R.string.password, keyboard = KeyboardType.Password,
            placeholder = if (passwordSaved) stringResource(R.string.password_saved_placeholder) else "",
            transformation = PasswordVisualTransformation(), limit = 1024)
        HelperText(stringResource(if (host.keyUri.isNotEmpty()) R.string.ssh_key_passphrase_hint else R.string.ssh_password_optional_hint))
        if (passwordSaved) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                HelperText(stringResource(if (host.keyUri.isNotEmpty()) R.string.ssh_passphrase_saved_status else R.string.password_saved_status), Modifier.weight(1f))
                TextButton(onClearPassword, enabled = !busy) { Text(stringResource(if (host.keyUri.isNotEmpty()) R.string.ssh_clear_saved_passphrase else R.string.clear_saved_password)) }
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Checkbox(rememberPassword, { rememberPassword = it }, enabled = !busy)
            Text(stringResource(if (host.keyUri.isNotEmpty()) R.string.ssh_save_passphrase else R.string.save_password), fontSize = 13.sp)
        }
        HelperText(stringResource(when {
            host.keyUri.isNotEmpty() && rememberPassword -> R.string.ssh_passphrase_will_save_hint
            host.keyUri.isNotEmpty() -> R.string.ssh_passphrase_not_saved_hint
            rememberPassword -> R.string.password_will_save_hint
            else -> R.string.password_not_saved_hint
        }))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Checkbox(desktop, { desktop = it }, enabled = !busy && attachmentLabel == null)
            Text(stringResource(R.string.connect_pebrel), fontSize = 13.sp)
        }
        if (attachmentLabel != null) HelperText(stringResource(R.string.remote_attach_target, attachmentLabel))
        if (desktop) {
            HelperText(stringResource(R.string.desktop_setup_hint))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Checkbox(input, { input = it }, enabled = !busy)
                Text(stringResource(R.string.allow_input), fontSize = 13.sp)
            }
        }
        Spacer(Modifier.height(20.dp))
        ConnectionButton(stringResource(R.string.connect), enabled = !busy) {
            val secret = password.takeIf { it.isNotEmpty() }?.toCharArray()
            onConnect(secret, desktop, input, rememberPassword)
        }
    }
}

@Composable
fun HostTrustForm(request: TrustRequest, onAnswer: (Boolean) -> Unit) {
    AlertDialog(onDismissRequest = { onAnswer(false) }, title = { Text(stringResource(R.string.verify_host)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(request.host.name)
                HelperText("${request.host.address}:${request.host.port}")
                HelperText(stringResource(R.string.verify_hint))
                SelectionContainer {
                    Text(request.fingerprint, fontFamily = LocalTerminalFont.current, fontSize = 12.sp, lineHeight = 20.sp)
                }
            }
        }, confirmButton = { TextButton({ onAnswer(true) }) { Text(stringResource(R.string.trust_connect)) } },
        dismissButton = { TextButton({ onAnswer(false) }) { Text(stringResource(R.string.cancel)) } })
}
