package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.DesktopFailureKind

@Composable
fun operationErrorText(code: String): String {
    DesktopFailureKind.entries.firstOrNull { it.code == code }?.let { return desktopFailureText(it) }
    return when (code) {
        "credential_load_failed" -> stringResource(R.string.credential_load_failed)
        "credential_save_failed", "host_save_failed" -> stringResource(R.string.credential_save_failed)
        "credential_clear_failed" -> stringResource(R.string.credential_clear_failed)
        "credential_missing" -> stringResource(R.string.credential_missing)
        "relay_connection_failed" -> desktopFailureText(DesktopFailureKind.UNKNOWN)
        "relay_storage_failed" -> stringResource(R.string.relay_storage_failed)
        "invalid_relay_invite" -> stringResource(R.string.invalid_relay_invite)
        "desktop_read_failed" -> stringResource(R.string.desktop_read_failed)
        "delivery_unknown" -> stringResource(R.string.delivery_unknown)
        "input_rejected" -> stringResource(R.string.input_rejected)
        else -> stringResource(R.string.operation_failed_hint)
    }
}
