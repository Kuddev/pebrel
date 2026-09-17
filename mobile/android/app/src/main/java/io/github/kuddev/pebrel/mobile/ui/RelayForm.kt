package io.github.kuddev.pebrel.mobile.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
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
    var showImport by remember { mutableStateOf(false) }
    var invalid by remember { mutableStateOf(false) }
    fun importInvitation(text: String) {
        runCatching { RelayProfile.parse(text) }.onSuccess {
            name = it.name; url = it.url; device = it.device; token = it.token
            pin = it.tlsPin.orEmpty(); mode = it.mode
            pasted = ""; showImport = false; invalid = false
        }.onFailure { invalid = true }
    }
    val scanner = rememberLauncherForActivityResult(ScanContract()) { result -> result.contents?.let(::importInvitation) }
    val prompt = stringResource(R.string.pair_scan_prompt)
    ConnectionForm(stringResource(R.string.computer_connect), onCancel) {
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Button({ scanner.launch(ScanOptions().setDesiredBarcodeFormats(ScanOptions.QR_CODE)
                .setPrompt(prompt).setBeepEnabled(false).setOrientationLocked(false)) }, modifier = Modifier.weight(1f)) {
                Glyph(R.drawable.ic_qr, color = MaterialTheme.colorScheme.onPrimary)
                Text(stringResource(R.string.pair_scan), Modifier.padding(start = 7.dp))
            }
            OutlinedButton({ showImport = !showImport }, modifier = Modifier.weight(1f)) {
                Text(stringResource(R.string.pair_import))
            }
        }
        HelperText(stringResource(R.string.pair_helper_hint), Modifier.padding(top = 10.dp, bottom = 16.dp))
        if (showImport) {
            ConnectionField(pasted, { pasted = it }, R.string.relay_invite, limit = 8192,
                transformation = PasswordVisualTransformation())
            TextButton({ importInvitation(pasted) }, enabled = pasted.isNotBlank()) { Text(stringResource(R.string.pair_read)) }
        }
        ConnectionSegments(listOf("lan" to stringResource(R.string.pair_lan), "relay" to stringResource(R.string.pair_relay)),
            mode, { mode = it }, Modifier.fillMaxWidth())
        Column(Modifier.padding(top = 12.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            ConnectionField(name, { name = it }, R.string.pair_computer_name, limit = 80)
            ConnectionField(url, { url = it }, R.string.pair_server_url, keyboard = KeyboardType.Uri, placeholder = "wss://", limit = 2048)
            ConnectionField(device, { device = it }, R.string.pair_device_id, limit = 64)
            ConnectionField(token, { token = it }, R.string.pair_access_key, limit = 43, transformation = PasswordVisualTransformation())
            if (mode == "lan" || pin.isNotEmpty()) ConnectionField(pin, { pin = it }, R.string.pair_certificate_pin, limit = 51)
            if (mode == "relay") HelperText(stringResource(R.string.relay_trust_hint))
            if (invalid) Text(stringResource(R.string.pair_invalid), color = MaterialTheme.colorScheme.error)
            ConnectionButton(stringResource(R.string.connect), enabled = name.isNotBlank() && url.isNotBlank() && device.isNotBlank() && token.isNotBlank()) {
                val data = JSONObject().put("version", 1).put("name", name).put("url", url).put("device", device)
                    .put("token", token).put("mode", mode).apply { if (pin.isNotBlank()) put("tlsPin", pin) }
                runCatching { RelayProfile.parse(data.toString()) }.onSuccess { onConnect(it.toJson().toString()) }.onFailure { invalid = true }
            }
            if (mode == "relay") {
                HorizontalDivider(Modifier.padding(vertical = 4.dp))
                NavigationRow(R.drawable.ic_server, stringResource(R.string.pair_deploy_server), onClick = onDeploy)
            }
        }
    }
}
