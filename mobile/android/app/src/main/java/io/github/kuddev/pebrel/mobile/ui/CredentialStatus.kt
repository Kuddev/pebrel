package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import io.github.kuddev.pebrel.mobile.R

@Composable
fun credentialErrorText(code: String): String = when (code) {
    "credential_load_failed" -> stringResource(R.string.credential_load_failed)
    "credential_save_failed", "host_save_failed" -> stringResource(R.string.credential_save_failed)
    "credential_clear_failed" -> stringResource(R.string.credential_clear_failed)
    "credential_missing" -> stringResource(R.string.credential_missing)
    "desktop_tab_create_failed" -> stringResource(R.string.desktop_tab_create_failed)
    "desktop_tab_close_failed" -> stringResource(R.string.desktop_tab_close_failed)
    "desktop_tab_changed" -> stringResource(R.string.desktop_tab_changed)
    "desktop_tab_control_unavailable" -> stringResource(R.string.desktop_tab_control_unavailable)
    "delivery_unknown" -> stringResource(R.string.delivery_unknown)
    "desktop_read_failed" -> stringResource(R.string.desktop_read_failed)
    "desktop_connection_failed" -> stringResource(R.string.desktop_connection_failed)
    "relay_connection_failed" -> stringResource(R.string.relay_connection_failed)
    "relay_storage_failed" -> stringResource(R.string.relay_storage_failed)
    "input_rejected" -> stringResource(R.string.input_rejected)
    else -> stringResource(R.string.operation_failed)
}
