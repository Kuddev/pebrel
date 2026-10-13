package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.RelayProfile

@Composable
internal fun ComputerAddressDialog(profile: RelayProfile, onDismiss: () -> Unit, onConnect: (RelayProfile) -> Unit) {
    var address by remember(profile) { mutableStateOf(profile.url.removePrefix("wss://")) }
    var invalid by remember(profile) { mutableStateOf(false) }
    AlertDialog(onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.computer_edit_address)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                OutlinedTextField(address, { address = it; invalid = false }, Modifier.fillMaxWidth(),
                    label = { Text(stringResource(R.string.computer_address)) }, singleLine = true, isError = invalid,
                    supportingText = if (invalid) ({ Text(stringResource(R.string.computer_address_invalid)) }) else null)
                HelperText(stringResource(R.string.computer_address_hint))
            }
        },
        confirmButton = { TextButton({
            val replacement = runCatching { profile.atLanAddress(address) }.getOrNull()
            if (replacement == null) invalid = true else onConnect(replacement)
        }, enabled = address.isNotBlank()) { Text(stringResource(R.string.connect)) } },
        dismissButton = { TextButton(onDismiss) { Text(stringResource(R.string.cancel)) } })
}
