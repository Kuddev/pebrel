package io.github.kuddev.pebrel.mobile.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.*
import io.github.kuddev.pebrel.mobile.session.SessionRepository
import kotlinx.coroutines.*
import java.util.UUID

/** Native service management is separate from terminal and PC-pairing lifecycles. */
@Composable
fun RelayDeploymentFlow(repository: SessionRepository, onCancel: () -> Unit) {
    val context = LocalContext.current
    val hosts by repository.hosts.collectAsStateWithLifecycle()
    val credentials by repository.savedCredentials.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    val owner = remember { repository.beginSshOperation() }
    DisposableEffect(owner) { onDispose { repository.endSshOperation(owner) } }
    var selectedId by remember { mutableStateOf(hosts.firstOrNull()?.id) }
    var address by remember { mutableStateOf("") }
    var user by remember { mutableStateOf("root") }
    var password by remember { mutableStateOf("") }
    var sshPort by remember { mutableStateOf("22") }
    var servicePort by remember { mutableStateOf("443") }
    var advertisedAddress by remember { mutableStateOf("") }
    var advanced by remember { mutableStateOf(false) }
    var manual by remember { mutableStateOf(false) }
    var choosing by remember { mutableStateOf(false) }
    var result by remember { mutableStateOf<RelayServiceResult?>(null) }
    var stage by remember { mutableStateOf<String?>(null) }
    var installProgress by remember { mutableStateOf<RelayInstallProgress?>(null) }
    var failure by remember { mutableStateOf<Int?>(null) }
    var job by remember { mutableStateOf<Job?>(null) }
    var running by remember { mutableStateOf(false) }
    var operation by remember { mutableStateOf<Any?>(null) }
    var confirmRemove by remember { mutableStateOf(false) }
    var purge by remember { mutableStateOf(false) }
    var exportFeedback by remember { mutableStateOf<Int?>(null) }
    var exportText by remember { mutableStateOf("") }
    val busy = running
    val host = hosts.find { it.id == selectedId }
    val endpoint = runCatching { parseSshEndpoint(address, user) }.getOrNull()
    val savedPassword = credentials.isNotEmpty() && host != null && repository.hasSavedPassword(host)
    val valid = (host != null || endpoint != null && (sshPort.toIntOrNull() ?: 0) in 1..65535) &&
        (password.isNotEmpty() || savedPassword) && (servicePort.toIntOrNull() ?: 0) in 1..65535
    val saveFile = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri ->
        val text = exportText
        exportText = ""
        if (uri != null) scope.launch {
            exportFeedback = try {
                withContext(Dispatchers.IO) { checkNotNull(context.contentResolver.openOutputStream(uri)).use { it.write(text.toByteArray()) } }
                R.string.deploy_export_saved
            } catch (cancelled: CancellationException) { throw cancelled }
            catch (_: Exception) { R.string.deploy_export_failed }
        }
    }
    fun execute(action: RelayServiceAction) {
        if (!valid || busy) return
        val selected = host ?: HostProfile(UUID.randomUUID().toString(), address, checkNotNull(endpoint).address, sshPort.toInt(), endpoint.user)
        val entered = password.takeIf(String::isNotEmpty)?.toCharArray()
        val relayAddress = advertisedAddress.ifBlank { selected.address }
        val port = servicePort.toInt()
        val removeConfiguration = purge
        password = ""
        failure = null
        stage = "connecting"
        installProgress = if (action == RelayServiceAction.INSTALL) RelayInstallProgress() else null
        running = true
        val token = Any()
        operation = token
        job = scope.launch {
            var secret = entered
            try {
                if (secret == null) secret = repository.loadSavedPassword(selected)
                if (secret == null) throw RelayServiceFailure("missing_credentials")
                result = NativeRelayDeployment.execute(context, selected, checkNotNull(secret),
                    { h, fingerprint -> repository.verifySshOperation(owner, h, fingerprint) },
                    action, relayAddress, port, removeConfiguration) { update ->
                    scope.launch { if (operation === token) {
                        stage = update.stage
                        installProgress = installProgress?.advance(update)
                    } }
                }
                installProgress = installProgress?.copy(finished = true)
                stage = when {
                    result?.state?.ready == true -> "ready"
                    result?.state?.running == true -> "not_ready"
                    result?.state?.installed == true -> "stopped"
                    action == RelayServiceAction.UNINSTALL -> "uninstalled"
                    else -> "not_installed"
                }
            } catch (_: TimeoutCancellationException) {
                failure = R.string.ssh_error_timeout; stage = "failed"
                installProgress = installProgress?.copy(failed = true)
            }
            catch (cancelled: CancellationException) {
                installProgress = installProgress?.copy(cancelled = true)
                throw cancelled
            }
            catch (error: Exception) {
                failure = serviceErrorText(error); stage = "failed"
                installProgress = installProgress?.copy(failed = true)
            }
            finally { secret?.fill('\u0000'); operation = null; running = false; job = null }
        }
    }
    fun resetStatus() { result = null; stage = null; installProgress = null; failure = null; exportFeedback = null }
    ConnectionForm(stringResource(R.string.pair_deploy_server), onCancel) {
        Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            HelperText(stringResource(R.string.service_intro))
            if (!busy) {
                if (hosts.isNotEmpty()) OutlinedButton({ choosing = true }, modifier = Modifier.fillMaxWidth()) {
                    Text(host?.name ?: stringResource(R.string.deploy_choose_host))
                }
                if (host == null) {
                    ConnectionField(address, { address = it; resetStatus() }, R.string.host_address, keyboard = KeyboardType.Uri)
                    ConnectionField(user, { user = it; resetStatus() }, R.string.username)
                }
                ConnectionField(password, { password = it }, R.string.password, keyboard = KeyboardType.Password,
                    placeholder = if (savedPassword) stringResource(R.string.password_saved_placeholder) else "",
                    transformation = PasswordVisualTransformation(), limit = 1024)
                TextButton({ advanced = !advanced }) { Text(stringResource(R.string.service_advanced)) }
                if (advanced) {
                    if (host == null) ConnectionField(sshPort, { sshPort = it; resetStatus() }, R.string.port, keyboard = KeyboardType.Number, limit = 5)
                    ConnectionField(servicePort, { servicePort = it; resetStatus() }, R.string.service_port, keyboard = KeyboardType.Number, limit = 5)
                    ConnectionField(advertisedAddress, { advertisedAddress = it; resetStatus() }, R.string.service_address, keyboard = KeyboardType.Uri,
                        placeholder = host?.address ?: endpoint?.address.orEmpty())
                    HelperText(stringResource(R.string.service_address_hint))
                }
            }
            Card(Modifier.fillMaxWidth()) {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Text(stringResource(R.string.service_title))
                    Text(stringResource(serviceStageText(stage)), color = MaterialTheme.colorScheme.onSurfaceVariant)
                    if (busy && installProgress == null) LinearProgressIndicator(Modifier.fillMaxWidth())
                    if (installProgress == null) failure?.let { Text(stringResource(it), color = MaterialTheme.colorScheme.error) }
                    if (!busy) {
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Button({ execute(RelayServiceAction.INSTALL) }, enabled = valid) { Text(stringResource(R.string.service_install)) }
                            TextButton({ execute(RelayServiceAction.STATUS) }, enabled = valid) { Text(stringResource(R.string.service_check)) }
                        }
                        if (result?.state?.installed == true) Row {
                            TextButton({ execute(if (result?.state?.running == true) RelayServiceAction.STOP else RelayServiceAction.START) }, enabled = valid) {
                                Text(stringResource(if (result?.state?.running == true) R.string.service_stop else R.string.service_start))
                            }
                            TextButton({ purge = false; confirmRemove = true }, enabled = valid) { Text(stringResource(R.string.service_uninstall)) }
                        }
                    }
                }
            }
            HelperText(stringResource(R.string.service_requirements))
            if (result?.access != null) {
                HelperText(stringResource(R.string.service_pc_next))
                OutlinedButton({ exportText = result?.access.orEmpty(); saveFile.launch("pebrel-relay-access.json") }) {
                    Text(stringResource(R.string.deploy_save_pc))
                }
                HelperText(stringResource(R.string.service_export_private))
            }
            exportFeedback?.let { HelperText(stringResource(it)) }
            if (busy) TextButton({ job?.cancel(); stage = "cancelled" }) { Text(stringResource(R.string.cancel)) }
            installProgress?.let { RelayInstallSteps(it, failure) }
            if (!busy) TextButton({ manual = !manual }) { Text(stringResource(R.string.service_manual_commands)) }
            if (manual) {
                HelperText(stringResource(R.string.service_manual_hint))
                val relayAddress = runCatching {
                    NativeRelayDeployment.validatedAddress(advertisedAddress.ifBlank { host?.address ?: endpoint?.address.orEmpty() })
                }.getOrDefault("SERVER_IP")
                val relayPort = servicePort.toIntOrNull()?.takeIf { it in 1..65535 }?.toString() ?: "PORT"
                SelectionContainer { Text("sh install.sh '$relayAddress' $relayPort\n/opt/pebrel-relay/pebrel-relay service-status",
                    fontFamily = LocalTerminalFont.current, style = MaterialTheme.typography.bodySmall) }
            }
        }
    }
    if (choosing) AlertDialog(onDismissRequest = { choosing = false }, title = { Text(stringResource(R.string.deploy_choose_host)) },
        text = { Column { hosts.forEach { entry -> TextButton({ selectedId = entry.id; password = ""; choosing = false; resetStatus() }) { Text(entry.name) } }
            TextButton({ selectedId = null; password = ""; choosing = false; resetStatus() }) { Text(stringResource(R.string.deploy_manual)) } } },
        confirmButton = { TextButton({ choosing = false }) { Text(stringResource(R.string.close)) } })
    if (confirmRemove) AlertDialog(onDismissRequest = { confirmRemove = false }, title = { Text(stringResource(R.string.service_uninstall)) },
        text = { Column { Text(stringResource(R.string.service_uninstall_hint)); Row { Checkbox(purge, { purge = it }); Text(stringResource(R.string.service_purge)) } } },
        confirmButton = { TextButton({ confirmRemove = false; execute(RelayServiceAction.UNINSTALL) }) { Text(stringResource(R.string.service_uninstall)) } },
        dismissButton = { TextButton({ confirmRemove = false }) { Text(stringResource(R.string.cancel)) } })
}

internal fun serviceStageText(stage: String?): Int = when (stage) {
    "failed" -> R.string.service_operation_failed
    "connecting" -> R.string.establishing_connection
    "checking" -> R.string.deploy_prerequisites
    "uploading" -> R.string.service_uploading
    "uploaded" -> R.string.service_uploaded
    "installing" -> R.string.service_installing
    "initializing" -> R.string.deploy_initializing
    "starting" -> R.string.service_starting
    "verifying" -> R.string.service_verifying
    "ready" -> R.string.deploy_done
    "stopping", "removing" -> R.string.service_removing
    "stopped" -> R.string.service_stopped
    "uninstalled" -> R.string.service_uninstalled
    "cancelled" -> R.string.service_cancelled
    "not_installed" -> R.string.service_not_installed
    "not_ready" -> R.string.service_not_ready
    else -> R.string.service_unchecked
}

internal fun serviceErrorText(error: Exception): Int = when ((error as? RelayServiceFailure)?.code) {
    "asset_missing", "binary_integrity_failed" -> R.string.service_asset_error
    "linux_systemd_required", "systemd_247_required", "systemd_239_required", "unsupported_arch", "supported_init_required" -> R.string.service_system_error
    "openrc_supervisor_required" -> R.string.service_openrc_error
    "remote_tools_missing" -> R.string.service_tools_error
    "service_command_failed", "service_command_output_limit" -> R.string.service_manager_error
    "service_command_timeout", "operation_timeout" -> R.string.ssh_error_timeout
    "unprivileged_account_required", "privilege_drop_failed" -> R.string.service_privilege_error
    "root_required", "permission_denied" -> R.string.service_permission_error
    "managed_file_changed", "installation_conflict", "explicit_update_required", "service_manager_changed",
    "configuration_directory_not_empty", "symlink_installation_path", "invalid_ownership_manifest" -> R.string.service_conflict
    "file_or_service_not_found" -> R.string.service_missing
    "service_not_ready", "port_in_use" -> R.string.service_not_ready
    "invalid_address" -> R.string.service_address_error
    else -> when (classifySshFailure(error)) {
        SshFailureKind.AUTH -> R.string.ssh_error_auth
        SshFailureKind.TIMEOUT -> R.string.ssh_error_timeout
        SshFailureKind.HOST_KEY_CHANGED -> R.string.ssh_error_host_key
        SshFailureKind.TRUST_REJECTED -> R.string.ssh_error_trust
        else -> R.string.service_failed
    }
}
