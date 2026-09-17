package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.HostProfile
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
    var password by remember { mutableStateOf("") }
    val passwordIsSaved = passwordSaved && initial != null && initial.address == address.trim() &&
        initial.port == port.toIntOrNull() && initial.user == user.trim()
    var rememberPassword by rememberSaveable(initial?.id) { mutableStateOf(passwordSaved || initial == null) }
    val valid = name.isNotBlank() && address.isNotBlank() && !address.any { it.isWhitespace() } &&
        !address.contains("://") && user.isNotBlank() && (port.toIntOrNull() ?: 0) in 1..65535
    fun profile() = HostProfile(initial?.id ?: UUID.randomUUID().toString(), name.trim(), address.trim(), port.toInt(), user.trim(),
        icon = icon, group = group)
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
                ConnectionField(address, { address = it }, R.string.host_address, Modifier.weight(1f),
                    keyboard = KeyboardType.Uri, placeholder = "server.example.com")
                ConnectionField(port, { port = it }, R.string.port, Modifier.width(82.dp), KeyboardType.Number, limit = 5)
            }
            ConnectionField(user, { user = it }, R.string.username, limit = 40)
            SegmentRow(R.string.authentication) {
                ConnectionSegments(listOf("password" to stringResource(R.string.auth_password), "key" to stringResource(R.string.auth_key)),
                    "password", {}, Modifier.weight(1f), disabled = setOf("key"))
            }
            ConnectionField(password, { password = it }, R.string.credential_password,
                keyboard = KeyboardType.Password, placeholder = if (passwordIsSaved) stringResource(R.string.password_saved_placeholder) else "",
                transformation = PasswordVisualTransformation(), limit = 1024)
            Column {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(rememberPassword, { rememberPassword = it }, enabled = !busy)
                    Text(stringResource(R.string.save_password), fontSize = 13.sp, modifier = Modifier.weight(1f))
                    if (passwordIsSaved) {
                        TextButton({
                            password = ""
                            onClearPassword()
                        }, enabled = !busy) { Text(stringResource(R.string.clear_saved_password), fontSize = 12.sp) }
                    }
                }
                HelperText(stringResource(
                    when {
                        passwordIsSaved && rememberPassword -> R.string.password_saved_hint
                        rememberPassword -> R.string.password_will_save_hint
                        else -> R.string.password_not_saved_hint
                    },
                ), Modifier.padding(start = 12.dp))
            }
            SegmentRow(R.string.host_group) {
                ConnectionSegments(listOf("production" to stringResource(R.string.group_production), "development" to stringResource(R.string.group_development)),
                    group, { group = it }, Modifier.weight(1f))
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
            Button({ submit(true) }, enabled = valid && !busy && (password.isNotEmpty() || passwordIsSaved),
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
              onClearPassword: () -> Unit, onConnect: (CharArray?, Boolean, Boolean, Boolean) -> Unit) {
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
                HelperText("${host.user}@${host.address}:${host.port}", Modifier.padding(top = 6.dp))
            }
        }
        Spacer(Modifier.height(24.dp))
        ConnectionField(password, { password = it }, R.string.password, keyboard = KeyboardType.Password,
            placeholder = if (passwordSaved) stringResource(R.string.password_saved_placeholder) else "",
            transformation = PasswordVisualTransformation(), limit = 1024)
        if (passwordSaved) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                HelperText(stringResource(R.string.password_saved_status), Modifier.weight(1f))
                TextButton(onClearPassword, enabled = !busy) { Text(stringResource(R.string.clear_saved_password)) }
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Checkbox(rememberPassword, { rememberPassword = it }, enabled = !busy)
            Text(stringResource(R.string.save_password), fontSize = 13.sp)
        }
        HelperText(stringResource(if (rememberPassword) R.string.password_will_save_hint else R.string.password_not_saved_hint))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Checkbox(desktop, { desktop = it }, enabled = !busy)
            Text(stringResource(R.string.connect_pebrel), fontSize = 13.sp)
        }
        if (desktop) {
            HelperText(stringResource(R.string.desktop_setup_hint))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Checkbox(input, { input = it }, enabled = !busy)
                Text(stringResource(R.string.allow_input), fontSize = 13.sp)
            }
        }
        Spacer(Modifier.height(20.dp))
        ConnectionButton(stringResource(R.string.connect), enabled = !busy && (password.isNotEmpty() || passwordSaved)) {
            val secret = password.takeIf { it.isNotEmpty() }?.toCharArray()
            onConnect(secret, desktop, input, rememberPassword)
        }
    }
}

@Composable
fun HostTrustForm(request: TrustRequest, onAnswer: (Boolean) -> Unit) {
    ConnectionForm(stringResource(R.string.verify_host), { onAnswer(false) }) {
        Text(request.host.name, fontSize = 16.sp)
        HelperText("${request.host.address}:${request.host.port}", Modifier.padding(top = 8.dp))
        HelperText(stringResource(R.string.verify_hint), Modifier.padding(top = 24.dp))
        Surface(Modifier.fillMaxWidth().padding(vertical = 20.dp), shape = MaterialTheme.shapes.small,
            color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = .4f)) {
            SelectionContainer {
                Text(request.fingerprint, Modifier.padding(15.dp), fontFamily = LocalTerminalFont.current, fontSize = 12.sp, lineHeight = 22.sp)
            }
        }
        ConnectionButton(stringResource(R.string.trust_connect)) { onAnswer(true) }
        Spacer(Modifier.height(12.dp))
        ConnectionButton(stringResource(R.string.cancel), primary = false) { onAnswer(false) }
    }
}
