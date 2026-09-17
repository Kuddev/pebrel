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
    else -> code
}
