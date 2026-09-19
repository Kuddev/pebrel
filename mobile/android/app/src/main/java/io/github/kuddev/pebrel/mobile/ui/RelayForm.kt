package io.github.kuddev.pebrel.mobile.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import io.github.kuddev.pebrel.mobile.R

@Composable
fun RelayForm(onCancel: () -> Unit, onConnect: (String) -> Unit) {
    var invite by remember { mutableStateOf("") }
    val scanPrompt = stringResource(R.string.scan_qr_prompt)
    val scanner = rememberLauncherForActivityResult(ScanContract()) { result ->
        result.contents?.let(onConnect)
    }
    AlertDialog(onDismissRequest = onCancel, title = { Text(stringResource(R.string.relay_connect)) }, text = {
        Column(Modifier.verticalScroll(rememberScrollState())) {
            Text(stringResource(R.string.relay_invite_hint))
            OutlinedButton(
                onClick = {
                    scanner.launch(ScanOptions().apply {
                        setDesiredBarcodeFormats(listOf(ScanOptions.QR_CODE))
                        setPrompt(scanPrompt)
                        setBeepEnabled(false)
                        setBarcodeImageEnabled(false)
                        setOrientationLocked(false)
                    })
                },
                modifier = Modifier.fillMaxWidth().padding(top = 16.dp),
            ) {
                Glyph(R.drawable.ic_qr, color = LocalContentColor.current)
                Spacer(Modifier.width(9.dp))
                Text(stringResource(R.string.scan_qr_invite))
            }
            Row(
                Modifier.fillMaxWidth().padding(top = 16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                HorizontalDivider(Modifier.weight(1f))
                Text(
                    stringResource(R.string.or_paste_invite),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 10.dp),
                )
                HorizontalDivider(Modifier.weight(1f))
            }
            OutlinedTextField(invite, { if (it.length <= 8192) invite = it },
                modifier = Modifier.padding(top = 12.dp).fillMaxWidth(),
                label = { Text(stringResource(R.string.relay_invite)) }, maxLines = 5,
                visualTransformation = PasswordVisualTransformation())
            Text(stringResource(R.string.relay_trust_hint), style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(top = 12.dp))
        }
    }, confirmButton = { TextButton(enabled = invite.isNotBlank(), onClick = { val value = invite; invite = ""; onConnect(value) }) { Text(stringResource(R.string.connect)) } },
        dismissButton = { TextButton(onClick = onCancel) { Text(stringResource(R.string.cancel)) } })
}
