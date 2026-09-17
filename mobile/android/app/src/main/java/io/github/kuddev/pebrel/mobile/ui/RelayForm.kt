package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import io.github.kuddev.pebrel.mobile.R

@Composable
fun RelayForm(onCancel: () -> Unit, onConnect: (String) -> Unit) {
    var invite by remember { mutableStateOf("") }
    AlertDialog(onDismissRequest = onCancel, title = { Text(stringResource(R.string.relay_connect)) }, text = {
        Column(Modifier.verticalScroll(rememberScrollState())) {
            Text(stringResource(R.string.relay_invite_hint))
            OutlinedTextField(invite, { if (it.length <= 8192) invite = it },
                modifier = Modifier.padding(top = 16.dp).fillMaxWidth(),
                label = { Text(stringResource(R.string.relay_invite)) }, maxLines = 5,
                visualTransformation = PasswordVisualTransformation())
            Text(stringResource(R.string.relay_trust_hint), style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(top = 12.dp))
        }
    }, confirmButton = { TextButton(enabled = invite.isNotBlank(), onClick = { val value = invite; invite = ""; onConnect(value) }) { Text(stringResource(R.string.connect)) } },
        dismissButton = { TextButton(onClick = onCancel) { Text(stringResource(R.string.cancel)) } })
}
