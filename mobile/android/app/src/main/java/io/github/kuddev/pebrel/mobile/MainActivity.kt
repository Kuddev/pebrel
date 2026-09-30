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
import io.github.kuddev.pebrel.mobile.git.DesktopGitTarget
import io.github.kuddev.pebrel.mobile.session.*
import io.github.kuddev.pebrel.mobile.ui.*
import kotlinx.coroutines.launch
import kotlinx.coroutines.CancellationException

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
    override fun onStart() {
        super.onStart()
        (application as PebrelApplication).sessions.foregroundChanged(true)
    }
    override fun onStop() {
        (application as PebrelApplication).sessions.foregroundChanged(false)
        super.onStop()
    }

    @OptIn(ExperimentalAnimationApi::class)
    @Composable
    private fun Workspace(repository: SessionRepository, target: Intent?) {
        val sessions by repository.sessions.collectAsStateWithLifecycle()
        val sftpTabs by repository.sftpTabs.collectAsStateWithLifecycle()
        val desktops by repository.desktops.collectAsStateWithLifecycle()
        val hosts by repository.hosts.collectAsStateWithLifecycle()
        val relays by repository.relays.collectAsStateWithLifecycle()
        val trust by repository.trust.collectAsStateWithLifecycle()
        val error by repository.error.collectAsStateWithLifecycle()
        val savedCredentials by repository.savedCredentials.collectAsStateWithLifecycle()
        val desktopOutput by repository.output.collectAsStateWithLifecycle()
        val credentialScope = rememberCoroutineScope()
        var credentialBusy by remember { mutableStateOf(false) }
        var page by rememberSaveable {
            mutableStateOf(if (repository.display.connectionOnboardingCompleted) "home" else "onboarding")
        }
        var selected by rememberSaveable { mutableStateOf("") }
        var desktopId by rememberSaveable { mutableStateOf("") }
        var paneId by rememberSaveable { mutableLongStateOf(-1L) }
        var windowId by rememberSaveable { mutableLongStateOf(-1L) }
        var paneProcess by rememberSaveable { mutableStateOf<Long?>(null) }
        var fileTabId by rememberSaveable { mutableStateOf("") }
        var sftpTabId by rememberSaveable { mutableStateOf("") }
        var sftpPaths by rememberSaveable { mutableStateOf(mapOf<String, String>()) }
        var chatKind by rememberSaveable { mutableStateOf("") }
        var chatSession by rememberSaveable { mutableStateOf("") }
        var gitCwd by rememberSaveable { mutableStateOf("") }
        var gitGeneration by rememberSaveable { mutableLongStateOf(-1L) }
        var pageDirection by rememberSaveable { mutableStateOf("forward") }
        var settingsInitial by rememberSaveable { mutableStateOf("") }
        var hostForm by rememberSaveable { mutableStateOf(false) }
        var editHost by remember { mutableStateOf<HostProfile?>(null) }
        var deleteHost by remember { mutableStateOf<HostProfile?>(null) }
        var addRelay by remember { mutableStateOf(false) }
        var deployRelay by remember { mutableStateOf(false) }
        var login by remember { mutableStateOf<HostProfile?>(null) }
        var retrySession by remember { mutableStateOf<String?>(null) }
        var pendingAttachment by remember { mutableStateOf<RemoteAttachment?>(null) }
        var switcher by remember { mutableStateOf(false) }
        val desktop = desktops.find { it.id == desktopId }
        val pane = desktop?.panes?.find { it.id == paneId && it.window == windowId }
        val fileTab = desktop?.tabs?.find { it.id == fileTabId && it.window == windowId }
        val motion = rememberPebrelMotion()
        fun showPage(destination: String) {
            pageDirection = "forward"
            page = destination
        }
        fun finishOnboarding() {
            if (page == "onboarding") {
                repository.display.completeConnectionOnboarding()
                showPage("home")
            }
        }
        fun openSession(id: String) { selected = id; showPage("terminal"); switcher = false }
        fun openDesktop(id: String) { desktopId = id; paneId = -1L; showPage("desktop"); switcher = false }
        fun openSftpTab(entry: SftpTab) {
            selected = entry.session; sftpTabId = entry.id; switcher = false; showPage("sftp_file")
        }
        fun openSftpFile(entry: SftpEntry) { openSftpTab(repository.openSftpTab(selected, entry)) }
        fun closeSftpTab(entry: SftpTab) {
            repository.closeSftpTab(entry.id)
            if (page == "sftp_file" && sftpTabId == entry.id) {
                val next = repository.sftpTabs.value.lastOrNull { it.session == entry.session }
                if (next == null) showPage("sftp") else openSftpTab(next)
            }
        }
        fun openPane(id: String, entry: DesktopPane) {
            paneProcess = desktops.find { it.id == id }?.runtimeProcess
            desktopId = id; windowId = entry.window; paneId = entry.id; showPage("pane"); switcher = false
        }
        fun openTab(id: String, entry: DesktopTab) {
            val owner = desktops.find { it.id == id } ?: return
            credentialScope.launch {
                try {
                    if (owner.allowInput && owner.status == "ready" && entry.id != null && !entry.active) repository.desktopTabs(id).focus(entry)
                    val terminal = entry.primaryPane
                    if (terminal != null) openPane(id, terminal)
                    else if (entry.readable) {
                        desktopId = id; windowId = entry.window; fileTabId = entry.id.orEmpty()
                        paneProcess = owner.runtimeProcess; switcher = false; showPage("file")
                    }
                } catch (cancelled: CancellationException) { throw cancelled }
                catch (error: Exception) { repository.error.value = tabFailureCode(error) }
            }
        }
        LaunchedEffect(desktopId, relays) {
            if (page in setOf("pane", "desktop", "git", "file", "conversation")) repository.restoreDesktop(desktopId)?.let { desktopId = it }
        }
        fun connectHost(host: HostProfile, previous: String? = null, attachment: RemoteAttachment? = null) {
            if (credentialBusy) return
            if (!repository.hasSavedPassword(host)) {
                retrySession = previous
                pendingAttachment = attachment
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
                        pendingAttachment = attachment
                        login = host
                    } else {
                        previous?.let(repository::closeTerminal)
                        openSession(repository.ssh(host, checkNotNull(secret), attachment))
                        secret = null
                    }
                } finally {
                    secret?.fill('\u0000')
                    credentialBusy = false
                }
            }
        }
        val openLocal = rememberLocalTerminalLauncher {
            val id = repository.local()
            finishOnboarding()
            openSession(id)
        }
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
            val destination = when {
                page == "sftp_file" -> "sftp"
                page == "sftp" -> "terminal"
                page in setOf("git", "conversation") && desktop != null -> "pane"
                page in setOf("pane", "file") && desktop != null -> "desktop"
                else -> "home"
            }
            pageDirection = "back"
            page = destination
        }
        BackHandler(page != "home" && page != "onboarding") { back() }
        LaunchedEffect(target) {
            if (target?.action == "OPEN_TASK") {
                desktopId = target.getStringExtra("desktop").orEmpty()
                windowId = target.getLongExtra("window", -1)
                paneId = target.getLongExtra("pane", -1)
                paneProcess = target.getLongExtra("process", -1).takeIf { it >= 0 }
                repository.restoreDesktop(desktopId)?.let { desktopId = it }
                showPage("pane")
            }
        }
        val paneFrame = desktopOutput.frame.takeIf {
            page == "pane" && desktopOutput.target == "$desktopId:$windowId:$paneId"
        }
        DesktopTerminalTheme(paneFrame, systemBars = true) {
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
                    "onboarding" -> ConnectionOnboardingScreen(
                        onComputer = { addRelay = true },
                        onSshHost = { editHost = null; hostForm = true },
                        onLocal = openLocal,
                        onLater = ::finishOnboarding,
                    )
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
                            ComputerRows(desktops, relays, ::openDesktop, { openDesktop(repository.connectRelay(it)) },
                                onAddRelay = { addRelay = true }, onForget = repository::forgetRelay)
                            HelperText(stringResource(R.string.connection_boundary), Modifier.padding(top = 20.dp))
                        }
                    }
                    "terminal" -> {
                        val session = sessions.find { it.id == selected }
                        if (session != null) LocalTerminalScreen(session, repository, ::back, { switcher = true },
                            onRetry = { session.host?.let { host ->
                                connectHost(hosts.find { it.id == host.id } ?: host, session.id, session.attachment)
                            } },
                            onEdit = { session.host?.let { host ->
                                editHost = hosts.find { it.id == host.id } ?: host; hostForm = true
                                repository.closeTerminal(session.id); back()
                            } },
                            onClose = { repository.closeTerminal(session.id); back() },
                            onFiles = session.files?.let { { showPage("sftp") } },
                            onAttachRemote = { entry -> session.host?.let { connectHost(it, attachment = entry) } })
                        else LaunchedEffect(selected) { showPage("home") }
                    }
                    "sftp" -> {
                        val session = sessions.find { it.id == selected }
                        if (session?.files != null) key(selected) {
                            SftpBrowserScreen(session, sftpPaths[selected] ?: ".", { sftpPaths = sftpPaths + (selected to it) }, ::back, ::openSftpFile)
                        } else LaunchedEffect(selected) { showPage("home") }
                    }
                    "sftp_file" -> {
                        val session = sessions.find { it.id == selected }
                        val entry = sftpTabs.find { it.id == sftpTabId && it.session == selected }
                        if (session?.files != null && entry != null) key(entry.id) {
                            SftpFileScreen(session, entry, ::back, { switcher = true }, ::openSftpFile, { closeSftpTab(entry) })
                        } else LaunchedEffect(selected, sftpTabId) { showPage(if (session?.files != null) "sftp" else "home") }
                    }
                    "pane" -> {
                        if (desktop != null && pane != null && (paneProcess == null || paneProcess == desktop.runtimeProcess)) DesktopTerminalScreen(desktop, pane, repository, ::back, { switcher = true },
                            onGit = {
                                gitCwd = pane.cwd; gitGeneration = desktop.connectionGeneration; paneProcess = desktop.runtimeProcess
                                showPage("git")
                            }, onConversation = {
                                chatKind = pane.agent?.kind.orEmpty(); chatSession = pane.agent?.session.orEmpty()
                                showPage("conversation")
                            })
                        else {
                            PageHeader(stringResource(R.string.computer_tabs), ::back)
                            HelperText(if (desktop?.status == "connecting") stringResource(R.string.desktop_reconnecting)
                                else stringResource(R.string.desktop_session_changed), Modifier.padding(22.dp))
                            if (desktop != null) TextButton({ openDesktop(desktop.id) }) { Text(stringResource(R.string.computer_tabs)) }
                        }
                    }
                    "git" -> GitScreen(DesktopGitTarget(desktopId, windowId, paneId, gitCwd, gitGeneration, paneProcess), repository, ::back)
                    "conversation" -> {
                        if (desktop != null && pane != null && paneProcess == desktop.runtimeProcess &&
                            chatKind in setOf("codex", "claude") && chatSession.isNotEmpty()) {
                            ConversationScreen(desktop, pane, ConversationIdentity(chatKind, chatSession), repository,
                                ::back, { switcher = true }, { openTab(desktopId, it) })
                        } else {
                            PageHeader(stringResource(R.string.chat_title), ::back)
                            HelperText(stringResource(R.string.chat_unavailable), Modifier.padding(22.dp))
                            if (chatSession.isEmpty() && pane?.agent?.kind == chatKind && pane.agent.session != null) {
                                LaunchedEffect(pane.agent.session) { chatSession = pane.agent.session.orEmpty() }
                            }
                        }
                    }
                    "file" -> {
                        if (desktop != null && fileTab?.readable == true && (paneProcess == null || paneProcess == desktop.runtimeProcess)) {
                            key(desktopId, fileTab.key) {
                                DesktopFileScreen(desktop, fileTab, repository, ::back, { switcher = true }, { openTab(desktopId, it) })
                            }
                        } else {
                            PageHeader(stringResource(R.string.computer_tabs), ::back)
                            HelperText(stringResource(R.string.desktop_session_changed), Modifier.padding(22.dp))
                            if (desktop != null) TextButton({ openDesktop(desktop.id) }) { Text(stringResource(R.string.computer_tabs)) }
                        }
                    }
                    "desktop" -> {
                        val profile = desktop?.relayProfile ?: relays.find { it.id == desktop?.host?.id }
                        DesktopTabsPage(desktop, repository, ::back, { openTab(desktopId, it) }, { openPane(desktopId, it) },
                            onRetry = profile?.let { saved -> { openDesktop(repository.connectRelay(saved)) } },
                        ) { repository.closeDesktop(desktopId); back() }
                    }
                    else -> {
                        HomeHeader({ settingsInitial = ""; showPage("settings") }, { settingsInitial = "notices"; showPage("settings") },
                            showNotices = !isHomeEmpty(sessions, hosts, desktops, relays))
                        HomeScreen(sessions, hosts, desktops, relays, ::openSession, { switcher = true }, { showPage("hosts") },
                            { connectHost(it) }, { editHost = it; hostForm = true }, { deleteHost = it }, { editHost = null; hostForm = true },
                            ::openDesktop, { openDesktop(repository.connectRelay(it)) }, { showPage("computers") }, { addRelay = true },
                            openLocal, { deployRelay = true }, ::openPane, ::openTab)
                    }
                }
                }
                }
            }
        }
        }
        if (switcher) AlertDialog(onDismissRequest = { switcher = false }, title = { Text(stringResource(R.string.all_sessions)) }, text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                val cards = sessionCards(sessions, desktops)
                if (cards.isEmpty()) HelperText(stringResource(R.string.no_sessions))
                cards.forEach { card ->
                    val local = card.local
                    if (local != null) {
                        NavigationRow(R.drawable.ic_terminal, local.title, statusLabel(local.status)) { openSession(local.id) }
                        sftpTabs.filter { it.session == local.id }.forEach { file ->
                            SftpTabRow(file, local.host?.name ?: local.title, page == "sftp_file" && sftpTabId == file.id,
                                { openSftpTab(file) }, { closeSftpTab(file) })
                        }
                    }
                    else card.desktop?.let { computer ->
                        NavigationRow(card.tab?.let(::tabIcon) ?: R.drawable.ic_monitor,
                            card.tab?.displayTitle ?: card.pane?.displayTitle ?: computer.host.name,
                            "${computer.host.name} · ${statusLabel(computer.status)}") {
                            card.tab?.let { openTab(computer.id, it) }
                                ?: card.pane?.let { openPane(computer.id, it) } ?: openDesktop(computer.id)
                        }
                    }
                }
                desktop?.let { computer ->
                    NavigationRow(R.drawable.ic_plus, stringResource(R.string.tab_manage), computer.host.name) { openDesktop(computer.id) }
                }
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
                        if (connect) connectionPassword = repository.passwordForConnection(host, password)
                        if (connect && connectionPassword == null) repository.error.value = "credential_missing"
                        else if (repository.saveHostWithCredentials(host, password, rememberPassword)) {
                            hostForm = false
                            finishOnboarding()
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
            repository.importRelay(invitation)?.let { finishOnboarding(); openDesktop(it); addRelay = false }
        }, onDeploy = { addRelay = false; deployRelay = true })
        if (deployRelay) RelayDeploymentFlow(repository, onCancel = { deployRelay = false })
        login?.let { host -> LoginForm(
            host = host,
            onCancel = { login = null; retrySession = null; pendingAttachment = null },
            attachmentLabel = pendingAttachment?.title,
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
                        connectionPassword = repository.passwordForConnection(host, password)
                        if (connectionPassword == null) repository.error.value = "credential_missing"
                        else if (repository.saveHostWithCredentials(host, password, rememberPassword)) {
                            val secret = checkNotNull(connectionPassword)
                            retrySession?.let(repository::closeTerminal)
                            retrySession = null
                            if (computer) openDesktop(repository.connectDesktop(host, secret, input))
                            else openSession(repository.ssh(host, secret, pendingAttachment))
                            pendingAttachment = null
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
