package io.github.kuddev.pebrel.mobile.ui

import android.content.ClipData
import android.content.ClipboardManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
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

/** A deployment has its own cancelable lifecycle and never occupies a terminal session. */
@Composable
fun RelayDeploymentFlow(repository: SessionRepository, onCancel: () -> Unit, onConnect: (String) -> Unit) {
    val context = LocalContext.current
    val hosts by repository.hosts.collectAsStateWithLifecycle()
    val credentials by repository.savedCredentials.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    val owner = remember { repository.beginSshOperation() }
    DisposableEffect(owner) { onDispose { repository.endSshOperation(owner) } }
    var source by remember { mutableStateOf(if (hosts.isEmpty()) "manual" else "saved") }
    var selectedId by remember { mutableStateOf(hosts.firstOrNull()?.id.orEmpty()) }
    var choosingHost by remember { mutableStateOf(false) }
    var address by remember { mutableStateOf("") }
    var user by remember { mutableStateOf("root") }
    var sshPort by remember { mutableStateOf("22") }
    var password by remember { mutableStateOf("") }
    var domain by remember { mutableStateOf("") }
    var httpsPort by remember { mutableStateOf("443") }
    var httpPort by remember { mutableStateOf("80") }
    var directory by remember { mutableStateOf("~/.pebrel-relay") }
    var computerName by remember { mutableStateOf("") }
    var allowInput by remember { mutableStateOf(false) }
    var running by remember { mutableStateOf(false) }
    var job by remember { mutableStateOf<Job?>(null) }
    var progress by remember { mutableStateOf(RelayDeploymentStage.VALIDATING) }
    var error by remember { mutableStateOf<RelayDeploymentErrorCode?>(null) }
    var result by remember { mutableStateOf<RelayDeploymentResult?>(null) }
    var exportText by remember { mutableStateOf("") }
    var exportFeedback by remember { mutableStateOf<Int?>(null) }
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
    val savedHost = hosts.find { it.id == selectedId }
    val endpoint = runCatching { parseSshEndpoint(address, user) }.getOrNull()
    val usesSavedPassword = source == "saved" && savedHost != null && credentials.isNotEmpty() && repository.hasSavedPassword(savedHost)
    val valid = (if (source == "saved") savedHost != null else endpoint != null && (sshPort.toIntOrNull() ?: 0) in 1..65535) &&
        (password.isNotEmpty() || usesSavedPassword) && domain.isNotBlank() && computerName.isNotBlank() &&
        (httpsPort.toIntOrNull() ?: 0) in 1..65535 && (httpPort.toIntOrNull() ?: 0) in 1..65535 && directory.isNotBlank()
    fun dismiss() { job?.cancel(); repository.endSshOperation(owner); onCancel() }
    ConnectionForm(stringResource(R.string.pair_deploy_server), ::dismiss) {
        val completed = result
        when {
            completed != null -> {
                GroupHeading(stringResource(R.string.deploy_done))
                HelperText(stringResource(R.string.deploy_pc_next))
                HelperText(completed.mobileProfile.url, Modifier.padding(vertical = 12.dp))
                OutlinedButton({ exportText = completed.desktopConfig; saveFile.launch(completed.desktopConfigFileName) }, Modifier.fillMaxWidth()) {
                    Text(stringResource(R.string.deploy_save_pc))
                }
                OutlinedButton({ exportText = completed.mobileInvitation; saveFile.launch(completed.mobileInvitationFileName) }, Modifier.fillMaxWidth()) {
                    Text(stringResource(R.string.deploy_save_phone))
                }
                TextButton({
                    context.getSystemService(ClipboardManager::class.java).setPrimaryClip(ClipData.newPlainText("Pebrel", completed.desktopCommand))
                    exportFeedback = R.string.deploy_command_copied
                }) { Text(stringResource(R.string.deploy_copy_command)) }
                exportFeedback?.let { HelperText(stringResource(it), Modifier.padding(vertical = 8.dp)) }
                ConnectionButton(stringResource(R.string.connect)) { onConnect(completed.mobileInvitation) }
            }
            running -> {
                Column(Modifier.fillMaxWidth().padding(vertical = 32.dp), verticalArrangement = Arrangement.spacedBy(18.dp)) {
                    LinearProgressIndicator(Modifier.fillMaxWidth())
                    Text(stringResource(deploymentStageText(progress)))
                    HelperText(stringResource(R.string.deploy_wait_hint))
                    OutlinedButton(::dismiss) { Text(stringResource(R.string.cancel)) }
                }
            }
            else -> {
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    ConnectionSegments(listOf("saved" to stringResource(R.string.deploy_existing_host), "manual" to stringResource(R.string.deploy_manual)),
                        source, { source = it; password = "" }, Modifier.fillMaxWidth(), disabled = if (hosts.isEmpty()) setOf("saved") else emptySet())
                    if (source == "saved") {
                        NavigationRow(R.drawable.ic_server, savedHost?.name ?: stringResource(R.string.deploy_choose_host),
                            savedHost?.endpointLabel.orEmpty(), onClick = { choosingHost = true })
                    } else {
                        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                            ConnectionField(address, { value ->
                                address = value
                                if ('@' in value) runCatching { parseSshEndpoint(value, user) }.onSuccess { address = it.address; user = it.user }
                            }, R.string.host_address, Modifier.weight(1f), keyboard = KeyboardType.Uri)
                            ConnectionField(sshPort, { sshPort = it }, R.string.port, Modifier.width(82.dp), KeyboardType.Number, limit = 5)
                        }
                        ConnectionField(user, { user = it }, R.string.username, limit = 80)
                    }
                    ConnectionField(password, { password = it }, R.string.password, keyboard = KeyboardType.Password,
                        placeholder = if (usesSavedPassword) stringResource(R.string.password_saved_placeholder) else "",
                        transformation = PasswordVisualTransformation(), limit = 1024)
                    ConnectionField(domain, { domain = it }, R.string.deploy_domain, keyboard = KeyboardType.Uri, limit = 253)
                    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        ConnectionField(httpsPort, { httpsPort = it }, R.string.deploy_https_port, Modifier.weight(1f), KeyboardType.Number, limit = 5)
                        ConnectionField(httpPort, { httpPort = it }, R.string.deploy_http_port, Modifier.weight(1f), KeyboardType.Number, limit = 5)
                    }
                    ConnectionField(directory, { directory = it }, R.string.deploy_directory, limit = 240)
                    ConnectionField(computerName, { computerName = it }, R.string.pair_computer_name, limit = 80)
                    Row {
                        Checkbox(allowInput, { allowInput = it })
                        Text(stringResource(R.string.allow_input), Modifier.padding(top = 12.dp))
                    }
                    HelperText(stringResource(R.string.deploy_requirements))
                    error?.let { Text(stringResource(deploymentErrorText(it)), color = MaterialTheme.colorScheme.error) }
                    ConnectionButton(stringResource(R.string.deploy_start), enabled = valid) {
                        val host = if (source == "saved") checkNotNull(savedHost) else HostProfile(UUID.randomUUID().toString(), address,
                            checkNotNull(endpoint).address, sshPort.toInt(), endpoint.user)
                        val request = RelayDeploymentRequest(domain.trim(), httpsPort.toInt(), httpPort.toInt(), directory.trim(), computerName.trim(), allowInput)
                        val entered = password.takeIf { it.isNotEmpty() }?.toCharArray()
                        password = ""; error = null; running = true
                        job = scope.launch {
                            var secret: CharArray? = entered
                            try {
                                if (secret == null) secret = repository.loadSavedPassword(host)
                                if (secret == null) throw RelayDeploymentException(RelayDeploymentErrorCode.MISSING_CREDENTIALS)
                                result = RelayDeployment.deploy(context, host, checkNotNull(secret),
                                    { h, fingerprint -> repository.verifySshOperation(owner, h, fingerprint) }, request) { update ->
                                    scope.launch { progress = update.stage }
                                }
                            } catch (cancelled: CancellationException) { throw cancelled }
                            catch (failure: RelayDeploymentException) { error = failure.code }
                            catch (_: Exception) { error = RelayDeploymentErrorCode.UNKNOWN }
                            finally { secret?.fill('\u0000'); running = false }
                        }
                    }
                }
            }
        }
    }
    if (choosingHost) AlertDialog(onDismissRequest = { choosingHost = false }, title = { Text(stringResource(R.string.deploy_choose_host)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) { hosts.forEach { host -> NavigationRow(R.drawable.ic_server, host.name, host.endpointLabel) {
                selectedId = host.id; password = ""; choosingHost = false
            } } }
        }, confirmButton = { TextButton({ choosingHost = false }) { Text(stringResource(R.string.cancel)) } })
}

private fun deploymentStageText(stage: RelayDeploymentStage): Int = when (stage) {
    RelayDeploymentStage.VALIDATING -> R.string.deploy_validating
    RelayDeploymentStage.CONNECTING -> R.string.establishing_connection
    RelayDeploymentStage.CHECKING_PREREQUISITES -> R.string.deploy_prerequisites
    RelayDeploymentStage.PREPARING, RelayDeploymentStage.UPLOADING, RelayDeploymentStage.EXTRACTING -> R.string.deploy_uploading
    RelayDeploymentStage.INITIALIZING -> R.string.deploy_initializing
    RelayDeploymentStage.STARTING -> R.string.deploy_starting
    RelayDeploymentStage.HEALTH_CHECK -> R.string.deploy_health
    RelayDeploymentStage.COMPLETE -> R.string.deploy_done
}

private fun deploymentErrorText(code: RelayDeploymentErrorCode): Int = when (code) {
    RelayDeploymentErrorCode.MISSING_CREDENTIALS -> R.string.credential_missing
    RelayDeploymentErrorCode.SSH_AUTH -> R.string.ssh_error_auth
    RelayDeploymentErrorCode.SSH_TIMEOUT -> R.string.ssh_error_timeout
    RelayDeploymentErrorCode.SSH_HOST_KEY_CHANGED -> R.string.ssh_error_host_key
    RelayDeploymentErrorCode.SSH_TRUST_REJECTED -> R.string.ssh_error_trust
    RelayDeploymentErrorCode.SSH_FAILED -> R.string.ssh_error_network
    RelayDeploymentErrorCode.INVALID_INPUT -> R.string.deploy_invalid
    RelayDeploymentErrorCode.DOCKER_MISSING, RelayDeploymentErrorCode.DOCKER_UNAVAILABLE,
    RelayDeploymentErrorCode.COMPOSE_MISSING -> R.string.deploy_docker_required
    RelayDeploymentErrorCode.INSTALL_DIRECTORY_NOT_EMPTY, RelayDeploymentErrorCode.UNSUPPORTED_PROJECT_LAYOUT -> R.string.deploy_directory_conflict
    RelayDeploymentErrorCode.HEALTH_FAILED, RelayDeploymentErrorCode.HEALTH_CLIENT_MISSING -> R.string.deploy_health_failed
    RelayDeploymentErrorCode.CANCELLED -> R.string.cancel
    else -> R.string.deploy_failed
}
