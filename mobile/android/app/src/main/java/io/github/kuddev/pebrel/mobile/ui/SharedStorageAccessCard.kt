package io.github.kuddev.pebrel.mobile.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.session.SharedStorageAccess
import java.io.File

/** Inline gate for the local-terminal entry point; callers decide where to place it. */
@Composable
fun LocalTerminalStorageCard(modifier: Modifier = Modifier, onReady: () -> Unit = {}) {
    val context = LocalContext.current
    var state by remember { mutableStateOf(SharedStorageAccess.state(context)) }
    fun refresh() {
        state = SharedStorageAccess.state(context)
        if (state is SharedStorageAccess.State.Ready) onReady()
    }
    val runtimePermissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) { refresh() }
    val settingsLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.StartActivityForResult(),
    ) { refresh() }

    Column(
        modifier.fillMaxWidth().workspaceFrame().padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(stringResource(R.string.shared_storage_title), fontSize = 16.sp)
        when (val current = state) {
            is SharedStorageAccess.State.Ready -> {
                Text(stringResource(R.string.shared_storage_ready), color = MaterialTheme.colorScheme.primary, fontSize = 13.sp)
                Text(
                    stringResource(R.string.shared_storage_location, current.directory.absolutePath),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    fontSize = 12.sp,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            is SharedStorageAccess.State.RuntimePermissionRequired -> {
                HelperText(stringResource(R.string.shared_storage_required))
                HelperText(stringResource(R.string.shared_storage_explanation))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = { runtimePermissionLauncher.launch(current.permissions.toTypedArray()) }) {
                        Text(stringResource(R.string.shared_storage_grant))
                    }
                    TextButton(onClick = { settingsLauncher.launch(SharedStorageAccess.appDetailsIntent(context)) }) {
                        Text(stringResource(R.string.shared_storage_open_settings))
                    }
                }
            }
            is SharedStorageAccess.State.AllFilesAccessRequired -> {
                HelperText(stringResource(R.string.shared_storage_required))
                HelperText(stringResource(R.string.shared_storage_all_files_explanation))
                val intent = SharedStorageAccess.allFilesAccessIntent(context)
                Button(
                    enabled = intent != null,
                    onClick = { intent?.let { settingsLauncher.launch(it) } },
                ) { Text(stringResource(R.string.shared_storage_grant)) }
            }
            is SharedStorageAccess.State.Unavailable -> {
                val message = when (current.reason) {
                    SharedStorageAccess.Reason.MEDIA_UNAVAILABLE -> R.string.shared_storage_unavailable
                    SharedStorageAccess.Reason.DIRECTORY_NOT_WRITABLE,
                    SharedStorageAccess.Reason.DIRECTORY_CREATION_FAILED -> R.string.shared_storage_directory_error
                }
                HelperText(stringResource(message))
                TextButton(onClick = ::refresh) { Text(stringResource(R.string.shared_storage_check_again)) }
            }
        }
    }
}

/**
 * Returns a click handler for a local terminal. The handler supplies a verified shared
 * directory immediately, or opens the permission gate and waits for the user's decision.
 */
@Composable
fun rememberLocalTerminalLauncher(onReady: (File) -> Unit): () -> Unit {
    val context = LocalContext.current
    var gateVisible by remember { mutableStateOf(false) }
    if (gateVisible) {
        SharedStorageAccessDialog(
            onReady = { directory ->
                gateVisible = false
                onReady(directory)
            },
            onDismiss = { gateVisible = false },
        )
    }
    return {
        when (val current = SharedStorageAccess.state(context)) {
            is SharedStorageAccess.State.Ready -> onReady(current.directory)
            else -> gateVisible = true
        }
    }
}

/** Permission gate used by [rememberLocalTerminalLauncher]; denied requests remain visible. */
@Composable
fun SharedStorageAccessDialog(onReady: (File) -> Unit, onDismiss: () -> Unit) {
    val context = LocalContext.current
    var state by remember { mutableStateOf(SharedStorageAccess.state(context)) }
    fun refresh() {
        state = SharedStorageAccess.state(context)
    }
    val runtimePermissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) { refresh() }
    val settingsLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.StartActivityForResult(),
    ) { refresh() }

    LaunchedEffect(state) {
        (state as? SharedStorageAccess.State.Ready)?.directory?.let(onReady)
    }
    val current = state
    if (current !is SharedStorageAccess.State.Ready) AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.shared_storage_title)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                when (current) {
                    is SharedStorageAccess.State.RuntimePermissionRequired -> {
                        HelperText(stringResource(R.string.shared_storage_required))
                        HelperText(stringResource(R.string.shared_storage_explanation))
                        Button(
                            modifier = Modifier.fillMaxWidth(),
                            onClick = { runtimePermissionLauncher.launch(current.permissions.toTypedArray()) },
                        ) { Text(stringResource(R.string.shared_storage_grant)) }
                        TextButton(
                            modifier = Modifier.fillMaxWidth(),
                            onClick = { settingsLauncher.launch(SharedStorageAccess.appDetailsIntent(context)) },
                        ) { Text(stringResource(R.string.shared_storage_open_settings)) }
                    }
                    is SharedStorageAccess.State.AllFilesAccessRequired -> {
                        HelperText(stringResource(R.string.shared_storage_required))
                        HelperText(stringResource(R.string.shared_storage_all_files_explanation))
                        val intent = SharedStorageAccess.allFilesAccessIntent(context)
                        Button(
                            modifier = Modifier.fillMaxWidth(),
                            enabled = intent != null,
                            onClick = { intent?.let { settingsLauncher.launch(it) } },
                        ) { Text(stringResource(R.string.shared_storage_grant)) }
                    }
                    is SharedStorageAccess.State.Unavailable -> {
                        val message = when (current.reason) {
                            SharedStorageAccess.Reason.MEDIA_UNAVAILABLE -> R.string.shared_storage_unavailable
                            SharedStorageAccess.Reason.DIRECTORY_NOT_WRITABLE,
                            SharedStorageAccess.Reason.DIRECTORY_CREATION_FAILED -> R.string.shared_storage_directory_error
                        }
                        HelperText(stringResource(message))
                        TextButton(
                            modifier = Modifier.fillMaxWidth(),
                            onClick = ::refresh,
                        ) { Text(stringResource(R.string.shared_storage_check_again)) }
                    }
                    is SharedStorageAccess.State.Ready -> Unit
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}
