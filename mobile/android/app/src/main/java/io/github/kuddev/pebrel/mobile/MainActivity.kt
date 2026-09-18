package io.github.kuddev.pebrel.mobile

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.ExperimentalAnimationApi
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.kuddev.pebrel.mobile.connection.*
import io.github.kuddev.pebrel.mobile.session.*
import io.github.kuddev.pebrel.mobile.ui.*
import kotlinx.coroutines.launch

/** Activity owns navigation only; transports, terminal state and drafts live in the application. */
class MainActivity : ComponentActivity() {
    private val launchTarget = mutableStateOf<Intent?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        launchTarget.value = intent
        setContent { PebrelTheme { Workspace((application as PebrelApplication).sessions, launchTarget.value) } }
    }

    override fun onNewIntent(intent: Intent) { super.onNewIntent(intent); launchTarget.value = intent }

    @OptIn(ExperimentalAnimationApi::class)
    @Composable
    private fun Workspace(repository: SessionRepository, target: Intent?) {
        val sessions by repository.sessions.collectAsStateWithLifecycle()
        val desktops by repository.desktops.collectAsStateWithLifecycle()
        val hosts by repository.hosts.collectAsStateWithLifecycle()
        val relays by repository.relays.collectAsStateWithLifecycle()
        val trust by repository.trust.collectAsStateWithLifecycle()
        val error by repository.error.collectAsStateWithLifecycle()
        val savedCredentials by repository.savedCredentials.collectAsStateWithLifecycle()
        val credentialScope = rememberCoroutineScope()
        var credentialBusy by remember { mutableStateOf(false) }
        var page by rememberSaveable { mutableStateOf("home") }
        var selected by rememberSaveable { mutableStateOf("") }
        var desktopId by rememberSaveable { mutableStateOf("") }
        var paneId by rememberSaveable { mutableLongStateOf(-1L) }
        var windowId by rememberSaveable { mutableLongStateOf(-1L) }
        var pageDirection by rememberSaveable { mutableStateOf("forward") }
        var settingsInitial by rememberSaveable { mutableStateOf("") }
        var hostForm by rememberSaveable { mutableStateOf(false) }
        var editHost by remember { mutableStateOf<HostProfile?>(null) }
        var deleteHost by remember { mutableStateOf<HostProfile?>(null) }
        var addRelay by remember { mutableStateOf(false) }
        var deployRelay by remember { mutableStateOf(false) }
        var login by remember { mutableStateOf<HostProfile?>(null) }
        var retrySession by remember { mutableStateOf<String?>(null) }
        var switcher by remember { mutableStateOf(false) }
        val desktop = desktops.find { it.id == desktopId }
        val pane = desktop?.panes?.find { it.id == paneId && it.window == windowId }
        val motion = rememberPebrelMotion()
        fun showPage(destination: String) {
            pageDirection = "forward"
            page = destination
        }
        fun openSession(id: String) { selected = id; showPage("terminal"); switcher = false }
        fun openDesktop(id: String) { desktopId = id; paneId = -1L; showPage("desktop"); switcher = false }
        fun openPane(id: String, entry: DesktopPane) {
            desktopId = id; windowId = entry.window; paneId = entry.id; showPage("pane"); switcher = false
        }
        fun connectHost(host: HostProfile, previous: String? = null) {
            if (credentialBusy) return
            if (!repository.hasSavedPassword(host)) {
                retrySession = previous
                login = host
                return
            }
            credentialBusy = true
            credentialScope.launch {
                var secret: CharArray? = null
                try {
                    secret = repository.loadSavedPassword(host)
                    if (secret == null) {
                        retrySession = previous
                        login = host
                    } else {
                        previous?.let(repository::closeTerminal)
                        openSession(repository.ssh(host, checkNotNull(secret)))
                        secret = null
                    }
                } finally {
                    secret?.fill('\u0000')
                    credentialBusy = false
                }
            }
        }
        val openLocal = rememberLocalTerminalLauncher { openSession(repository.local()) }
        var notificationRequested by rememberSaveable { mutableStateOf(false) }
        val notificationPermission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {}
        val hasLiveSessions = sessions.any { it.status == "ready" || it.status == "connecting" } ||
            desktops.any { it.status == "ready" || it.status == "connecting" }
        LaunchedEffect(hasLiveSessions) {
            if (hasLiveSessions && !notificationRequested && Build.VERSION.SDK_INT >= 33) {
                notificationRequested = true
                if (checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
                    notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
                }
            }
        }
        fun back() {
            val destination = if (page == "pane" && desktop != null) "desktop" else "home"
            pageDirection = "back"
            page = destination
        }
        BackHandler(page != "home") { back() }
        LaunchedEffect(target) {
            if (target?.action == "OPEN_TASK") {
                desktopId = target.getStringExtra("desktop").orEmpty()
                windowId = target.getLongExtra("window", -1)
                paneId = target.getLongExtra("pane", -1)
                showPage("pane")
            }
        }
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            Column(Modifier.fillMaxSize().systemBarsPadding().imePadding()) {
                AnimatedContent(
                    modifier = Modifier.fillMaxSize(),
                    targetState = page,
                    transitionSpec = { motion.pageTransition(if (pageDirection == "back") PebrelNavigationDirection.Backward else PebrelNavigationDirection.Forward) },
                    label = "workspace_page",
                ) { route ->
                Column(Modifier.fillMaxSize()) {
                when (route) {
                    "settings" -> key(settingsInitial) {
                        SettingsScreen(repository, ::back, { showPage("computers") }, { enabled ->
                            if (enabled) SessionService.start(this@MainActivity) else SessionService.stop(this@MainActivity)
                        }, settingsInitial)
                    }
                    "hosts" -> {
                        PageHeader(stringResource(R.string.ssh_hosts), ::back) {
                            GlyphButton(R.drawable.ic_plus, stringResource(R.string.add_ssh), { editHost = null; hostForm = true })
                        }
                        HostsScreen(hosts, { connectHost(it) }, { editHost = it; hostForm = true }, { deleteHost = it })
                    }
                    "computers" -> {
                        PageHeader(stringResource(R.string.computers), ::back)
                        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(22.dp)) {
                            ComputerRows(desktops, relays, ::openDesktop, { openDesktop(repository.connectRelay(it)) }, repository::forgetRelay)
                            if (desktops.none { it.hasConnected } && relays.isEmpty()) HelperText(stringResource(R.string.no_computers))
                            NavigationRow(R.drawable.ic_monitor, stringResource(R.string.relay_connect), onClick = { addRelay = true })
                            HelperText(stringResource(R.string.connection_boundary), Modifier.padding(top = 20.dp))
                        }
                    }
                    "terminal" -> {
                        val session = sessions.find { it.id == selected }
                        if (session != null) LocalTerminalScreen(session, repository, ::back, { switcher = true },
                            onRetry = { session.host?.let { host ->
                                connectHost(hosts.find { it.id == host.id } ?: host, session.id)
                            } },
                            onEdit = { session.host?.let { host ->
                                editHost = hosts.find { it.id == host.id } ?: host; hostForm = true
                                repository.closeTerminal(session.id); back()
                            } },
                            onClose = { repository.closeTerminal(session.id); back() })
                        else LaunchedEffect(selected) { showPage("home") }
                    }
                    "pane" -> {
                        if (desktop != null && pane != null) DesktopTerminalScreen(desktop, pane, repository, ::back, { switcher = true })
                        else {
                            PageHeader(stringResource(R.string.computer_tabs), ::back)
                            HelperText(stringResource(R.string.device_unavailable), Modifier.padding(22.dp))
                        }
                    }
                    "desktop" -> {
                        PageHeader(stringResource(R.string.computers), ::back)
                        val profile = desktop?.relayProfile ?: relays.find { it.id == desktop?.host?.id }
                        DesktopScreen(desktop, { openPane(desktopId, it) },
                            onRetry = profile?.let { saved -> { openDesktop(repository.connectRelay(saved)) } },
                        ) { repository.closeDesktop(desktopId); back() }
                    }
                    else -> {
                        HomeHeader({ settingsInitial = ""; showPage("settings") }, { settingsInitial = "notices"; showPage("settings") })
                        HomeScreen(sessions, hosts, desktops, relays, ::openSession, { switcher = true }, { showPage("hosts") },
                            { connectHost(it) }, { editHost = it; hostForm = true }, { deleteHost = it }, { editHost = null; hostForm = true },
                            ::openDesktop, { openDesktop(repository.connectRelay(it)) }, { showPage("computers") }, { addRelay = true },
                            openLocal)
                    }
                }
                }
                }
            }
        }
        if (switcher) AlertDialog(onDismissRequest = { switcher = false }, title = { Text(stringResource(R.string.all_sessions)) }, text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                if (sessions.isEmpty() && desktops.all { it.panes.isEmpty() }) HelperText(stringResource(R.string.no_sessions))
                sessions.forEach { session -> NavigationRow(R.drawable.ic_terminal, session.title, statusLabel(session.status)) { openSession(session.id) } }
                desktops.forEach { computer -> computer.panes.forEach { entry ->
                    NavigationRow(R.drawable.ic_monitor, entry.title, computer.host.name) { openPane(computer.id, entry) }
                } }
            }
        }, confirmButton = { TextButton({ switcher = false }) { Text(stringResource(R.string.close)) } })
        if (hostForm) HostForm(
            initial = editHost,
            onCancel = { hostForm = false },
            passwordSaved = savedCredentials.isNotEmpty() && editHost?.let(repository::hasSavedPassword) == true,
            busy = credentialBusy,
            onClearPassword = {
                editHost?.let { host ->
                    credentialBusy = true
                    credentialScope.launch {
                        try { repository.clearHostPassword(host) } finally { credentialBusy = false }
                    }
                }
            },
            onSave = { host, password, rememberPassword, connect ->
                credentialBusy = true
                credentialScope.launch {
                    var connectionPassword: CharArray? = null
                    try {
                        if (connect) connectionPassword = password?.copyOf() ?: repository.loadSavedPassword(host)
                        if (connect && connectionPassword == null) repository.error.value = "credential_missing"
                        else if (repository.saveHostWithCredentials(host, password, rememberPassword)) {
                            hostForm = false
                            connectionPassword?.let { secret ->
                                val saved = repository.hosts.value.first { it.id == host.id }
                                openSession(repository.ssh(saved, secret))
                                connectionPassword = null
                            }
                        }
                    } finally {
                        password?.fill('\u0000')
                        connectionPassword?.fill('\u0000')
                        credentialBusy = false
                    }
                }
            },
        )
        deleteHost?.let { host -> AlertDialog(onDismissRequest = { deleteHost = null }, title = { Text(stringResource(R.string.delete_host)) },
            text = { Text(stringResource(R.string.delete_host_confirm, host.name)) },
            confirmButton = { TextButton({ repository.deleteHost(host); deleteHost = null }) { Text(stringResource(R.string.delete_host)) } },
            dismissButton = { TextButton({ deleteHost = null }) { Text(stringResource(R.string.cancel)) } }) }
        if (addRelay) RelayForm(onCancel = { addRelay = false }, onConnect = { invitation ->
            repository.importRelay(invitation)?.let { openDesktop(it); addRelay = false }
        }, onDeploy = { addRelay = false; deployRelay = true })
        if (deployRelay) RelayDeploymentFlow(repository, onCancel = { deployRelay = false })
        login?.let { host -> LoginForm(
            host = host,
            onCancel = { login = null; retrySession = null },
            passwordSaved = savedCredentials.isNotEmpty() && repository.hasSavedPassword(host),
            busy = credentialBusy,
            onClearPassword = {
                credentialBusy = true
                credentialScope.launch {
                    try { repository.clearHostPassword(host) } finally { credentialBusy = false }
                }
            },
            onConnect = { password, computer, input, rememberPassword ->
                credentialBusy = true
                credentialScope.launch {
                    var connectionPassword: CharArray? = null
                    try {
                        connectionPassword = password?.copyOf() ?: repository.loadSavedPassword(host)
                        if (connectionPassword == null) repository.error.value = "credential_missing"
                        else if (repository.saveHostWithCredentials(host, password, rememberPassword)) {
                            val secret = checkNotNull(connectionPassword)
                            retrySession?.let(repository::closeTerminal)
                            retrySession = null
                            if (computer) openDesktop(repository.connectDesktop(host, secret, input))
                            else openSession(repository.ssh(host, secret))
                            connectionPassword = null
                            login = null
                        }
                    } finally {
                        password?.fill('\u0000')
                        connectionPassword?.fill('\u0000')
                        credentialBusy = false
                    }
                }
            },
        ) }
        trust?.takeUnless { page == "terminal" && it.ownerId == selected }?.let { request ->
            HostTrustForm(request, repository::answerTrust)
        }
        error?.let { AlertDialog(onDismissRequest = { repository.error.value = null }, title = { Text(stringResource(R.string.operation_failed)) },
            text = { Text(operationErrorText(it)) }, confirmButton = { TextButton({ repository.error.value = null }) { Text(stringResource(R.string.close)) } }) }
    }
}
