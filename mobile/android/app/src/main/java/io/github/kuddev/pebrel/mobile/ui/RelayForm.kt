package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.RelayProfile
import org.json.JSONObject

@Composable
fun RelayForm(onCancel: () -> Unit, onConnect: (String) -> Unit, onDeploy: () -> Unit = {}) {
    var name by remember { mutableStateOf("") }
    var url by remember { mutableStateOf("") }
    var device by remember { mutableStateOf("") }
    var token by remember { mutableStateOf("") }
    var pin by remember { mutableStateOf("") }
    var mode by remember { mutableStateOf("lan") }
    var pasted by remember { mutableStateOf("") }
    var method by remember { mutableStateOf("scan") }
    var showManual by remember { mutableStateOf(false) }
    var invalid by remember { mutableStateOf(false) }
    fun importInvitation(text: String) {
        runCatching { RelayProfile.parse(text) }.onSuccess {
            invalid = false
            onConnect(it.toJson().toString())
        }.onFailure { invalid = true }
    }
    Dialog(onCancel, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            Column(Modifier.fillMaxSize().systemBarsPadding().imePadding()) {
                PageHeader(stringResource(R.string.computer_connect), onCancel)
                Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState())
                    .padding(start = 22.dp, end = 22.dp, top = 4.dp, bottom = 32.dp)) {
                    PairingProgress()
                    val setupHint = stringResource(R.string.pair_setup_hint)
                    val settingsPath = stringResource(R.string.pair_settings_path)
                    Text(buildAnnotatedString {
                        append(setupHint)
                        val start = setupHint.indexOf(settingsPath)
                        if (start >= 0) addStyle(SpanStyle(color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.Medium),
                            start, start + settingsPath.length)
                    }, fontSize = 14.sp, lineHeight = 22.sp,
                        modifier = Modifier.padding(start = 2.dp, end = 2.dp, bottom = 16.dp))
                    ConnectionSegments(listOf("scan" to stringResource(R.string.pair_tab_scan),
                        "code" to stringResource(R.string.pair_short_code), "paste" to stringResource(R.string.pair_tab_paste)),
                        method, { method = it; invalid = false }, Modifier.fillMaxWidth())
                    Spacer(Modifier.height(12.dp))
                    when (method) {
                        "scan" -> {
                            InlinePairingScanner(::importInvitation)
                            HelperText(stringResource(R.string.pair_qr_expiry), Modifier.padding(top = 8.dp, start = 2.dp))
                        }
                        "code" -> ShortCodePairing(onConnect)
                        "paste" -> {
                            val pasteLabel = stringResource(R.string.pair_import)
                            OutlinedTextField(pasted, { if (it.length <= 8192) { pasted = it; invalid = false } },
                                modifier = Modifier.fillMaxWidth().heightIn(min = 120.dp).semantics { contentDescription = pasteLabel },
                                minLines = 4, maxLines = 6,
                                shape = RoundedCornerShape(14.dp),
                                placeholder = { Text(stringResource(R.string.pair_paste_hint), fontSize = 12.sp, lineHeight = 20.sp) },
                                textStyle = MaterialTheme.typography.bodySmall.copy(fontFamily = LocalTerminalFont.current),
                                visualTransformation = PasswordVisualTransformation(), isError = invalid)
                            Spacer(Modifier.height(12.dp))
                            ConnectionButton(stringResource(R.string.pair_import_connect), enabled = pasted.isNotBlank(), primary = false) {
                                importInvitation(pasted)
                            }
                            TextButton({ showManual = !showManual }, modifier = Modifier.fillMaxWidth()) {
                                Text(stringResource(if (showManual) R.string.pair_hide_manual else R.string.pair_manual_setup))
                            }
                        }
                    }
                    if (invalid) Text(stringResource(R.string.pair_invalid), fontSize = 12.sp, lineHeight = 20.sp,
                        color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(top = 8.dp))
                    if (method == "paste" && showManual) Column(Modifier.padding(top = 12.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        ConnectionSegments(listOf("lan" to stringResource(R.string.pair_lan), "relay" to stringResource(R.string.pair_relay)),
                            mode, { mode = it }, Modifier.fillMaxWidth())
                        ConnectionField(name, { name = it }, R.string.pair_computer_name, limit = 80)
                        ConnectionField(url, { url = it }, R.string.pair_server_url, keyboard = KeyboardType.Uri, placeholder = "wss://", limit = 2048)
                        ConnectionField(device, { device = it }, R.string.pair_device_id, limit = 64)
                        ConnectionField(token, { token = it }, R.string.pair_access_key, limit = 43, transformation = PasswordVisualTransformation())
                        if (mode == "lan" || pin.isNotEmpty()) ConnectionField(pin, { pin = it }, R.string.pair_certificate_pin, limit = 51)
                        if (mode == "relay") HelperText(stringResource(R.string.relay_trust_hint))
                        ConnectionButton(stringResource(R.string.connect), enabled = name.isNotBlank() && url.isNotBlank() && device.isNotBlank() && token.isNotBlank()) {
                            val data = JSONObject().put("version", 1).put("name", name).put("url", url).put("device", device)
                                .put("token", token).put("mode", mode).apply { if (pin.isNotBlank()) put("tlsPin", pin) }
                            runCatching { RelayProfile.parse(data.toString()) }.onSuccess { onConnect(it.toJson().toString()) }.onFailure { invalid = true }
                        }
                    }
                    TextButton(onDeploy, modifier = Modifier.padding(top = 20.dp).heightIn(min = 48.dp),
                        contentPadding = PaddingValues(horizontal = 2.dp)) {
                        Text(stringResource(R.string.pair_relay_alternative), fontSize = 12.sp, lineHeight = 20.sp)
                    }
                }
            }
        }
    }
}

@Composable
private fun PairingProgress() {
    BoxWithConstraints(Modifier.fillMaxWidth().padding(start = 2.dp, end = 2.dp, top = 6.dp, bottom = 18.dp)) {
        if (maxWidth < 300.dp || LocalDensity.current.fontScale > 1.3f) {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                PairingStep(1, stringResource(R.string.pair_step_find), true)
                PairingStep(2, stringResource(R.string.pair_step_approve), false)
            }
        } else Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            PairingStep(1, stringResource(R.string.pair_step_find), true)
            HorizontalDivider(Modifier.weight(1f), color = MaterialTheme.colorScheme.outlineVariant)
            PairingStep(2, stringResource(R.string.pair_step_approve), false)
        }
    }
}

@Composable
private fun PairingStep(number: Int, label: String, active: Boolean) {
    val colors = MaterialTheme.colorScheme
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(7.dp)) {
        Box(Modifier.size(20.dp * LocalDensity.current.fontScale)
            .background(if (active) colors.primary else colors.background, CircleShape)
            .border(1.dp, if (active) colors.primary else colors.outline, CircleShape), contentAlignment = Alignment.Center) {
            Text(number.toString(), fontSize = 12.sp, lineHeight = 16.sp, fontWeight = FontWeight.Medium,
                color = if (active) colors.onPrimary else colors.onSurfaceVariant)
        }
        Text(label, fontSize = 12.sp, lineHeight = 18.sp, color = if (active) colors.onSurface else colors.onSurfaceVariant)
    }
}
