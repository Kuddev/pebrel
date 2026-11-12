package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import io.github.kuddev.pebrel.mobile.R

/** Only shown after authentication fails; leaves the terminal and its target in place. */
@Composable
internal fun SshRetryCredentials(key: Boolean, onDismiss: () -> Unit, onRetry: (CharArray) -> Unit) {
    var secret by remember { mutableStateOf("") }
    AlertDialog(onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.retry)) },
        text = { Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            HelperText(stringResource(R.string.ssh_retry_credentials_hint))
            ConnectionField(secret, { secret = it }, if (key) R.string.ssh_key_passphrase else R.string.password,
                keyboard = KeyboardType.Password, transformation = PasswordVisualTransformation(), limit = 1024)
        } },
        confirmButton = { TextButton({
            val password = secret.toCharArray()
            secret = ""
            onRetry(password)
        }) { Text(stringResource(R.string.retry)) } },
        dismissButton = { TextButton(onDismiss) { Text(stringResource(R.string.cancel)) } })
}
