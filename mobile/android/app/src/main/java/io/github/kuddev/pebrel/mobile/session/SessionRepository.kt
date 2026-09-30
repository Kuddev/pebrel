package io.github.kuddev.pebrel.mobile.session

import android.content.Context
import android.os.Handler
import android.os.Looper
import io.github.kuddev.pebrel.terminal.LocalPtyTransport
import io.github.kuddev.pebrel.terminal.SessionTransport
import io.github.kuddev.pebrel.terminal.TerminalSession
import io.github.kuddev.pebrel.terminal.TerminalCallbacks
import io.github.kuddev.pebrel.mobile.connection.*
import io.github.kuddev.pebrel.mobile.git.DesktopGit
import io.github.kuddev.pebrel.mobile.git.DesktopGitTarget
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

data class LocalSession(
    val id: String, val title: String, val source: String, val terminal: TerminalSession,
    val status: String = "connecting", val host: HostProfile? = null,
    val stage: SshStage = SshStage.NETWORK, val failure: SshFailureKind? = null, val hasConnected: Boolean = false,
    val files: SftpClient? = null,
    val attachment: RemoteAttachment? = null, val remote: RemoteInventory? = null,
    val discovering: Boolean = false, val discoveryFailed: Boolean = false,
)
data class DesktopWorkspace(val id: String, val host: HostProfile, val panes: List<DesktopPane> = emptyList(), val status: String = "connecting", val allowInput: Boolean = false, val transport: String = "SSH", val failure: DesktopFailureKind? = null,
                            val hasConnected: Boolean = false, val relayProfile: RelayProfile? = null,
                            val connectionGeneration: Long = 0, val runtimeProcess: Long? = null,
                            val pairingApproval: DesktopPairingApproval? = null,
                            val tabs: List<DesktopTab> = emptyList(), val windows: List<Long> = emptyList())
data class TrustRequest(val ownerId: String, val host: HostProfile, val fingerprint: String, val answer: CompletableFuture<Boolean>)
data class DesktopOutput(val target: String = "", val text: String = "", val loading: Boolean = false,
                         val frame: io.github.kuddev.pebrel.terminal.TerminalFrame? = null)

/** Application owns sessions; activities only attach views. Metadata never updates per cell. */
class SessionRepository(private val context: Context,
                        private val relayTransport: (RelayProfile) -> DesktopTransport = { RelayTransport(it) }) {
    val display = DisplayPreferences(context)
    private val main = Handler(Looper.getMainLooper())
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val preferences = context.getSharedPreferences("pebrel_mobile", Context.MODE_PRIVATE)
    private val credentialStore = HostCredentialStore(context)
    private val live = MutableStateFlow<List<LocalSession>>(emptyList())
    val sessions = live.asStateFlow()
    private val remoteFiles = MutableStateFlow<List<SftpTab>>(emptyList())
    val sftpTabs = remoteFiles.asStateFlow()
    private val computers = MutableStateFlow<List<DesktopWorkspace>>(emptyList())
    val desktops = computers.asStateFlow()
    private val relayStore = RelayProfileStore(context)
    private val relayWrites = Mutex()
    private val savedRelays = MutableStateFlow<List<RelayProfile>>(emptyList())
    private val restoredRelayIds = mutableMapOf<String, String>()
    val relays = savedRelays.asStateFlow()
    private val savedHosts = MutableStateFlow(loadHosts())
    val hosts = savedHosts.asStateFlow()
    val trust = MutableStateFlow<TrustRequest?>(null)
    val error = MutableStateFlow<String?>(null)
    private val savedCredentialIds = MutableStateFlow<Set<String>>(emptySet())
    val savedCredentials = savedCredentialIds.asStateFlow()
    val backgroundActive = MutableStateFlow(false)
    val theme = MutableStateFlow(preferences.getString("theme", "system") ?: "system")
    fun selectTheme(value: String) {
        theme.value = value
        scope.launch { preferences.edit().putString("theme", theme.value).apply() }
    }
    val output = MutableStateFlow(DesktopOutput())
    private val hostWrites = Mutex()
    private val credentialWrites = Mutex()
    private val sshOperations = java.util.concurrent.ConcurrentHashMap.newKeySet<String>()
    private val sshConnections = mutableMapOf<String, SshConnection>()
    private val discoveryJobs = mutableMapOf<String, Job>()
    private val pendingTrust = java.util.concurrent.ConcurrentHashMap<String, CompletableFuture<Boolean>>()
    private var readJob: Job? = null
    private var readGeneration = 0L
    private var desktopReader: DesktopReadScheduler? = null
    private val desktopClients = java.util.concurrent.ConcurrentHashMap<String, DesktopRuntimeClient>()
    private val desktopTransitions = mutableMapOf<String, DesktopTransitions>()
    private val conversationCache = ConversationCache()
    private var activeGit: DesktopGit? = null
    private var foreground = false
    private var lanDiscovery: PairingDiscovery? = null
    private var lanDiscoveryJob: Job? = null
    private val resumeChecks = mutableMapOf<String, Job>()
    private val network = DesktopNetworkMonitor(context, ::networkChanged)
    private val reconnect = DesktopReconnect(scope) { id ->
        val current = computers.value.find { it.id == id }
        if (foreground && current?.hasConnected == true && current.status != "ready" &&
            current.status !in setOf("connecting", "approval") && DesktopReconnect.retryable(current.failure)) {
            current.relayProfile?.let { connectRelay(it) }
        }
    }

    fun foregroundChanged(value: Boolean) {
        foreground = value
        reconnect.foreground(value)
        resumeChecks.values.forEach(Job::cancel)
        resumeChecks.clear()
        refreshLanDiscovery()
        if (!value) { network.stop(); return }
        network.start()
        for (desktop in computers.value.filter { it.hasConnected && it.relayProfile != null }) {
            val client = desktopClients[desktop.id]
            if (client == null && DesktopReconnect.retryable(desktop.failure)) reconnect.schedule(desktop.id, immediate = true)
            else if (client != null && desktop.status == "ready" && !client.recentlyActive) resumeChecks[desktop.id] = scope.launch {
                try { withTimeout(5000) { client.request("runtime.describe") } }
                catch (error: Exception) {
                    if (error is CancellationException && error !is TimeoutCancellationException) throw error
                    withContext(Dispatchers.Main) {
                        if (foreground) {
                            desktopFailed(desktop.id, client, DesktopFailureKind.NETWORK, immediate = true)
                            scope.launch { client.close() }
                        }
                    }
                }
            }
        }
    }

    internal fun networkChanged() {
        if (!foreground) return
        resumeChecks.values.forEach(Job::cancel)
        resumeChecks.clear()
        for (desktop in computers.value.filter {
            it.hasConnected && it.relayProfile != null && it.status != "approval" && DesktopReconnect.retryable(it.failure)
        }) {
            val client = desktopClients[desktop.id]
            if (client != null) {
                // 旧 TCP 不能迁移到新网络；结束旧连接代，避免等系统超时再恢复。
                desktopFailed(desktop.id, client, DesktopFailureKind.NETWORK, immediate = true)
                scope.launch { client.close() }
            } else reconnect.schedule(desktop.id, immediate = true)
        }
    }

    private fun refreshLanDiscovery() {
        val needed = foreground && computers.value.any {
            it.hasConnected && it.relayProfile?.canRediscover == true && it.status !in setOf("ready", "approval") &&
                DesktopReconnect.retryable(it.failure)
        }
        if (!needed) {
            val previous = lanDiscovery
            lanDiscovery = null
            lanDiscoveryJob?.cancel()
            lanDiscoveryJob = null
            previous?.close()
        } else if (lanDiscovery == null) {
            val discovery = PairingDiscovery(context)
            lanDiscovery = discovery
            lanDiscoveryJob = scope.launch(Dispatchers.Main.immediate) {
                discovery.state.collect { if (lanDiscovery === discovery) recoverLanComputers(it.computers) }
            }
            discovery.start()
        }
    }

    internal fun recoverLanComputers(discovered: List<PairingComputer>) {
        if (!foreground) return
        for (desktop in computers.value) {
            if (!desktop.hasConnected || desktop.status in setOf("ready", "approval") ||
                !DesktopReconnect.retryable(desktop.failure)) continue
            val profile = desktop.relayProfile ?: continue
            val computer = discovered.firstOrNull { it.pin == profile.tlsPin } ?: continue
            profile.discoveredAt(computer)?.let(::connectRelay)
        }
    }

    fun restoreDesktop(id: String): String? {
        val canonical = restoredRelayIds[id] ?: id
        if (computers.value.any { it.id == canonical }) return canonical
        return savedRelays.value.find { "relay:${it.id}" == canonical || "relay:${it.legacyId}" == id }?.let(::connectRelay)
    }
    private var renderOwner: String? = null
    private var renderToken: Any? = null
    private var redraw: (() -> Unit)? = null
    val drafts = MutableStateFlow<Map<String, String>>(emptyMap())
    private val recentCommands = MutableStateFlow<Map<String, List<String>>>(emptyMap())
    val commandHistory = recentCommands.asStateFlow()
    fun setDraft(id: String, text: String) { drafts.value = drafts.value + (id to text) }
    fun acknowledgeDraft(id: String, sent: String) {
        if (sent.isNotBlank()) {
            // Local composition history follows the live session, without writing shell input to disk.
            var remaining = 32_768
            val entries = (listOf(sent) + recentCommands.value[id].orEmpty())
                .distinct().take(40).takeWhile { remaining -= it.length; remaining >= 0 }
            recentCommands.value = recentCommands.value + (id to entries)
        }
        if (drafts.value[id] == sent) drafts.value = drafts.value - id
    }

    init {
        scope.launch {
            credentialWrites.withLock {
                runCatching { credentialStore.ids() }
                    .onSuccess { savedCredentialIds.value = it }
                    .onFailure { error.value = "credential_load_failed" }
            }
        }
        scope.launch {
            relayWrites.withLock {
                runCatching { relayStore.load() }.onSuccess { restored ->
                    val unique = RelayProfile.latestByComputer(restored)
                    withContext(Dispatchers.Main) {
                        restored.forEach { restoredRelayIds["relay:${it.legacyId}"] = "relay:${it.id}" }
                        savedRelays.value = RelayProfile.latestByComputer(unique + savedRelays.value)
                    }
                    // 迁移写盘失败也保留已经解密的可用记录，避免把存储错误变成重新扫码。
                    if (unique.size != restored.size) runCatching { relayStore.save(savedRelays.value) }
                        .onFailure { error.value = "relay_storage_failed" }
                }.onFailure { error.value = "relay_storage_failed" }
            }
        }
    }

    private fun loadHosts(): List<HostProfile> = runCatching {
        val rows = JSONArray(preferences.getString("hosts", "[]"))
        (0 until rows.length()).map { i -> rows.getJSONObject(i).let {
            HostProfile(it.getString("id"), it.getString("name"), it.getString("address"), it.getInt("port"), it.getString("user"),
                it.optString("fingerprint"), it.optString("icon", "term"), it.optString("group", "development"),
                SshSessionMode.entries.firstOrNull { mode -> mode.id == it.optString("session_mode") } ?: SshSessionMode.SHELL, it.optString("session_name"),
                it.optString("key_uri"), it.optString("key_name"))
        } }
    }.getOrDefault(emptyList())

    fun saveHost(host: HostProfile) {
        val previous = savedHosts.value.find { it.id == host.id }
        val fingerprint = if (previous != null && sameSshServer(previous, host)) previous.fingerprint else ""
        savedHosts.value = savedHosts.value.filterNot { it.id == host.id } + host.copy(fingerprint = fingerprint)
        persistHosts()
    }

    /** Credentials are scoped to both the saved identity and the login endpoint. */
    private fun credentialKey(host: HostProfile): String {
        // 保留旧密码记录的 AAD；密钥口令另按密钥文档隔离，避免切换认证方式后误用密码。
        val identity = "${host.id}\u0000${host.address}\u0000${host.port}\u0000${host.user}" +
            if (host.keyUri.isEmpty()) "" else "\u0000key\u0000${host.keyUri}"
        return java.security.MessageDigest.getInstance("SHA-256").digest(identity.toByteArray())
            .joinToString("") { "%02x".format(it.toInt() and 255) }
    }

    private fun sameSshServer(first: HostProfile, second: HostProfile): Boolean =
        first.port == second.port && runCatching {
            parseSshEndpoint(first.address, first.user).address == parseSshEndpoint(second.address, second.user).address
        }.getOrDefault(false)

    private fun sameSshLogin(first: HostProfile, second: HostProfile): Boolean =
        first.id == second.id && first.port == second.port && first.keyUri == second.keyUri && runCatching {
            parseSshEndpoint(first.address, first.user) == parseSshEndpoint(second.address, second.user)
        }.getOrDefault(false)

    private fun previousCredential(host: HostProfile): HostProfile? =
        savedHosts.value.firstOrNull { sameSshLogin(it, host) }

    fun hasSavedPassword(host: HostProfile): Boolean = savedCredentialIds.value.contains(credentialKey(host)) ||
        previousCredential(host)?.let { savedCredentialIds.value.contains(credentialKey(it)) } == true

    /** Completes only after encrypted credentials and host metadata reach storage. */
    suspend fun saveHostWithCredentials(host: HostProfile, password: CharArray?, rememberPassword: Boolean): Boolean =
        withContext(Dispatchers.IO) {
            hostWrites.withLock {
                credentialWrites.withLock {
                    try {
                        val previous = savedHosts.value.find { it.id == host.id }
                        if (host.keyUri.isNotEmpty()) {
                            val uri = android.net.Uri.parse(host.keyUri)
                            require(uri.scheme == "content")
                            context.contentResolver.takePersistableUriPermission(uri, android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION)
                        }
                        val key = credentialKey(host)
                        if (rememberPassword && password?.isNotEmpty() == true) credentialStore.save(key, password)
                        else if (!rememberPassword) credentialStore.clear(key)
                        else if (previous != null && credentialKey(previous) != key && sameSshLogin(previous, host)) {
                            // Canonicalizing user@host must retain the explicitly saved credential.
                            credentialStore.load(credentialKey(previous))?.let { saved ->
                                try { credentialStore.save(key, saved) } finally { saved.fill('\u0000') }
                            }
                        }
                        if (previous != null && credentialKey(previous) != key) credentialStore.clear(credentialKey(previous))
                        val fingerprint = if (previous != null && sameSshServer(previous, host)) previous.fingerprint else ""
                        val next = savedHosts.value.filterNot { it.id == host.id } + host.copy(fingerprint = fingerprint)
                        check(writeHostMetadata(next))
                        previous?.keyUri?.takeIf { it != host.keyUri }?.let { releaseKeyAccess(it, next) }
                        val ids = credentialStore.ids()
                        withContext(Dispatchers.Main) {
                            savedHosts.value = next
                            savedCredentialIds.value = ids
                        }
                        true
                    } catch (cancelled: CancellationException) { throw cancelled }
                    catch (_: Exception) { error.value = "credential_save_failed"; false }
                }
            }
        }

    suspend fun loadSavedPassword(host: HostProfile): CharArray? = withContext(Dispatchers.IO) {
        credentialWrites.withLock {
            try {
                credentialStore.load(credentialKey(host)) ?: previousCredential(host)?.let {
                    credentialStore.load(credentialKey(it))
                }
            }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (_: Exception) { error.value = "credential_load_failed"; null }
        }
    }

    suspend fun passwordForConnection(host: HostProfile, entered: CharArray?): CharArray? {
        if (entered != null) return entered.copyOf()
        // 无已存凭据才尝试免密；凭据读取失败仍阻止连接，不静默降级。
        return if (hasSavedPassword(host)) loadSavedPassword(host) else charArrayOf()
    }

    suspend fun clearHostPassword(host: HostProfile): Boolean = withContext(Dispatchers.IO) {
        credentialWrites.withLock {
            try {
                credentialStore.clear(credentialKey(host))
                savedCredentialIds.value = credentialStore.ids()
                true
            } catch (cancelled: CancellationException) { throw cancelled }
            catch (_: Exception) { error.value = "credential_clear_failed"; false }
        }
    }

    fun deleteHost(host: HostProfile) {
        savedHosts.value = savedHosts.value.filterNot { it.id == host.id }
        persistHosts()
        scope.launch {
            clearHostPassword(host)
            withContext(Dispatchers.IO) { releaseKeyAccess(host.keyUri, savedHosts.value) }
        }
    }

    private fun releaseKeyAccess(uri: String, hosts: List<HostProfile>) {
        if (uri.isEmpty() || hosts.any { it.keyUri == uri }) return
        try {
            context.contentResolver.releasePersistableUriPermission(android.net.Uri.parse(uri), android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION)
        } catch (_: SecurityException) { /* 文件提供方可能已经撤销授权。 */ }
    }

    private fun writeHostMetadata(hosts: List<HostProfile>): Boolean {
        val rows = JSONArray()
        hosts.forEach { h -> rows.put(JSONObject().put("id", h.id).put("name", h.name).put("address", h.address)
            .put("port", h.port).put("user", h.user).put("fingerprint", h.fingerprint).put("icon", h.icon).put("group", h.group)
            .put("session_mode", h.sessionMode.id).put("session_name", h.sessionName)
            .put("key_uri", h.keyUri).put("key_name", h.keyName)) }
        return preferences.edit().putString("hosts", rows.toString()).commit()
    }

    private fun persistHosts() {
        scope.launch {
            hostWrites.withLock {
                if (!writeHostMetadata(savedHosts.value)) error.value = "host_save_failed"
            }
        }
    }
    fun beginSshOperation(): String = UUID.randomUUID().toString().also { sshOperations.add(it) }
    fun endSshOperation(ownerId: String) { sshOperations.remove(ownerId); cancelTrust(ownerId) }
    fun verifySshOperation(ownerId: String, host: HostProfile, fingerprint: String): Boolean = verify(ownerId, host, fingerprint)

    private fun verify(ownerId: String, host: HostProfile, fingerprint: String): Boolean {
        val answer = CompletableFuture<Boolean>()
        pendingTrust[ownerId] = answer
        main.post {
            val active = live.value.any { it.id == ownerId && it.status == "connecting" } ||
                computers.value.any { it.id == ownerId && it.status == "connecting" } || sshOperations.contains(ownerId)
            if (!active || answer.isDone || trust.value != null) answer.complete(false)
            else trust.value = TrustRequest(ownerId, host, fingerprint, answer)
        }
        val accepted = runCatching { answer.get(60, TimeUnit.SECONDS) }.getOrDefault(false)
        pendingTrust.remove(ownerId, answer)
        main.post {
            if (trust.value?.answer === answer) trust.value = null
            if (accepted) {
                savedHosts.value = savedHosts.value.map { if (it.id == host.id && it.address == host.address && it.port == host.port) it.copy(fingerprint = fingerprint) else it }
                persistHosts()
            }
        }
        return accepted
    }
    private fun cancelTrust(ownerId: String) {
        pendingTrust.remove(ownerId)?.complete(false)
        if (trust.value?.ownerId == ownerId) { trust.value?.answer?.complete(false); trust.value = null }
    }
    fun answerTrust(accept: Boolean) { trust.value?.answer?.complete(accept); trust.value = null }
    fun attachRenderer(id: String, token: Any, callback: () -> Unit) {
        renderOwner = id; renderToken = token; redraw = callback
    }
    fun detachRenderer(id: String, token: Any) {
        if (renderOwner == id && renderToken === token) { renderOwner = null; renderToken = null; redraw = null }
    }
    @Volatile private var terminalColors: IntArray? = null
    fun setTerminalColors(value: IntArray) {
        terminalColors = value.copyOf()
        live.value.forEach { it.terminal.colors(value) }
    }
    fun local(): String = addTerminal("Term", "Local", LocalPtyTransport(
        LocalTerminalStorage.homePath(context), context.filesDir.resolve("terminal").absolutePath))
    fun ssh(host: HostProfile, password: CharArray, attachment: RemoteAttachment? = null): String {
        val id = UUID.randomUUID().toString()
        val command = attachment?.let(RemoteSessions::attachCommand)
        val connection = SshConnection(host, password, { h, fingerprint -> verify(id, h, fingerprint) }, progress = { stage ->
            main.post { update(id) { if (it.status == "connecting") it.copy(stage = stage) else it } }
        }, keySource = sshKeySource(context, host))
        val files = SftpClient(connection::sftp) { live.value.any { it.id == id && it.status == "ready" } }
        sshConnections[id] = connection
        return addTerminal(attachment?.title ?: host.name, "SSH", SshTerminalTransport(connection, command), id, host, files, attachment)
    }

    fun refreshRemoteSessions(id: String) {
        val connection = sshConnections[id] ?: return
        if (discoveryJobs[id]?.isActive == true || live.value.none { it.id == id && it.status == "ready" }) return
        update(id) { it.copy(discovering = true, discoveryFailed = false) }
        discoveryJobs[id] = scope.launch {
            val result = try { Result.success(RemoteSessions.discover(connection)) }
                catch (cancelled: CancellationException) { throw cancelled }
                catch (failure: Exception) { Result.failure(failure) }
            withContext(Dispatchers.Main) {
                if (sshConnections[id] !== connection) return@withContext
                val inventory = result.getOrNull()
                val host = live.value.find { it.id == id }?.host
                update(id) { it.copy(remote = inventory ?: it.remote, discovering = false, discoveryFailed = result.isFailure,
                    host = if (inventory == null || inventory.os == "term") it.host else it.host?.copy(icon = inventory.os)) }
                if (host != null && inventory != null && inventory.os != "term") {
                    val next = savedHosts.value.map { if (sameSshLogin(it, host) && it.icon == host.icon) it.copy(icon = inventory.os) else it }
                    if (next != savedHosts.value) { savedHosts.value = next; persistHosts() }
                }
            }
        }
    }

    suspend fun remoteWindows(id: String, session: RemoteSession): List<RemoteWindow> {
        val connection = checkNotNull(sshConnections[id])
        val result = withContext(Dispatchers.IO) { RemoteSessions.windows(connection, session) }
        check(sshConnections[id] === connection)
        return result
    }

    private fun addTerminal(title: String, source: String, transport: SessionTransport,
                            id: String = UUID.randomUUID().toString(), host: HostProfile? = null, files: SftpClient? = null,
                            attachment: RemoteAttachment? = null): String {
        val callbacks = object : TerminalCallbacks() {
            override fun onTextChanged(session: TerminalSession) { if (renderOwner == id) redraw?.invoke() }
            override fun onTitleChanged(session: TerminalSession) { update(id) { it.copy(title = session.title?.take(80) ?: it.title) } }
            override fun onTransportReady(session: TerminalSession) {
                update(id) { it.copy(status = "ready", hasConnected = true) }
                refreshRemoteSessions(id)
            }
            override fun onSessionFinished(session: TerminalSession) {
                sshConnections.remove(id)
                discoveryJobs.remove(id)?.cancel()
                cancelTrust(id)
                update(id) { it.copy(status = if (session.failure == null) "ended" else "failed",
                    failure = session.failureCause?.let(::classifySshFailure), discovering = false) }
                stopIdleService()
            }
            override fun onInputRejected(session: TerminalSession) { error.value = "input_rejected" }

        }
        val terminal = TerminalSession(transport, callbacks, display.state.value.scrollbackLines)
        live.value = live.value + LocalSession(id, title, source, terminal, host = host, files = files, attachment = attachment)
        SessionService.ensureStarted(context)
        terminal.start()
        terminalColors?.let { terminal.colors(it) }
        return id
    }
    private fun update(id: String, change: (LocalSession) -> LocalSession) { live.value = live.value.map { if (it.id == id) change(it) else it } }
    fun closeTerminal(id: String) {
        discoveryJobs.remove(id)?.cancel()
        sshConnections.remove(id)
        remoteFiles.value = remoteFiles.value.filterNot { it.session == id }
        cancelTrust(id)
        live.value.find { it.id == id }?.terminal?.finishIfRunning()
        live.value = live.value.filterNot { it.id == id }
        drafts.value = drafts.value - id
        recentCommands.value = recentCommands.value - id
        stopIdleService()
    }

    fun openSftpTab(session: String, file: SftpEntry): SftpTab {
        require(live.value.any { it.id == session && it.files != null })
        remoteFiles.value.find { it.session == session && it.file.path == file.path }?.let { return it }
        return SftpTab(UUID.randomUUID().toString(), session, file).also { remoteFiles.value = remoteFiles.value + it }
    }

    fun closeSftpTab(id: String) { remoteFiles.value = remoteFiles.value.filterNot { it.id == id } }
    fun importRelay(text: String): String? {
        return try {
            val profile = RelayProfile.parse(text)
            check(savedRelays.value.size < 64 || savedRelays.value.any { it.id == profile.id })
            connectRelay(profile)
        } catch (_: Exception) { error.value = "invalid_relay_invite"; null }
    }
    private fun persistRelays() {
        scope.launch {
            relayWrites.withLock {
                runCatching { relayStore.save(savedRelays.value) }.onFailure { error.value = "relay_storage_failed" }
            }
        }
    }
    fun forgetRelay(profile: RelayProfile) {
        computers.value.filter { it.host.id == profile.id }.forEach { closeDesktop(it.id) }
        savedRelays.value = savedRelays.value.filterNot { it.id == profile.id }
        desktopTransitions.remove("relay:${profile.id}")
        preferences.edit().remove("desktop_transitions_relay:${profile.id}").apply()
        persistRelays()
    }
    fun connectRelay(profile: RelayProfile): String {
        val previous = computers.value.find { it.host.id == profile.id }
        if (previous != null && previous.status in setOf("ready", "connecting", "approval") &&
            previous.relayProfile?.sameConnection(profile) == true) return previous.id
        val host = HostProfile(profile.id, profile.name, profile.url, 443, "", icon = profile.hostOs)
        return addDesktop(host, true, relayTransport(profile), if (profile.mode == "lan") "LAN" else "Relay",
            id = previous?.id ?: "relay:${profile.id}", relayProfile = profile)
    }
    fun connectDesktop(host: HostProfile, password: CharArray, allowInput: Boolean): String {
        val id = UUID.randomUUID().toString()
        return addDesktop(host, allowInput, SshDesktopTransport(SshConnection(host, password,
            { h, fingerprint -> verify(id, h, fingerprint) }, keySource = sshKeySource(context, host))), "SSH", id)
    }

    private fun addDesktop(host: HostProfile, allowInput: Boolean, transport: DesktopTransport, source: String,
                           id: String = UUID.randomUUID().toString(), relayProfile: RelayProfile? = null): String {
        reconnect.cancelPending(id)
        resumeChecks.remove(id)?.cancel()
        val previous = computers.value.find { it.id == id }
        val entry = if (previous == null) DesktopWorkspace(id, host, allowInput = false, transport = source, relayProfile = relayProfile,
            hasConnected = relayProfile != null && savedRelays.value.any { it.id == relayProfile.id })
            else previous.copy(host = host, status = "connecting", allowInput = false, failure = null, pairingApproval = null,
                connectionGeneration = previous.connectionGeneration + 1, transport = source, relayProfile = relayProfile)
        computers.value = if (previous == null) computers.value + entry else computers.value.map { if (it.id == id) entry else it }
        refreshLanDiscovery()
        SessionService.ensureStarted(context)
        val transitions = desktopTransitions.getOrPut(id) {
            DesktopTransitions(if (relayProfile == null) null else
                runCatching { preferences.getString("desktop_transitions_$id", null)?.let(::JSONObject) }.getOrNull())
        }
        val negotiatedInput = AtomicBoolean(false)
        lateinit var client: DesktopRuntimeClient
        client = DesktopRuntimeClient(transport, { snapshot ->
            val tabs = parseDesktopTabs(snapshot)
            val panes = tabs.flatMap { it.panes }
            val windows = snapshot.getJSONArray("windows").let { rows -> (0 until rows.length()).map { rows.getJSONObject(it).getLong("id") } }
            main.post {
                if (desktopClients[id] !== client) return@post
                if (computers.value.none { it.id == id }) return@post
                // 先核对连接代，再推进电脑的通知游标；旧连接不能吞掉新事件。
                val revision = transitions.revision
                val events = transitions.observe(snapshot)
                if (relayProfile != null && revision != transitions.revision) {
                    preferences.edit().putString("desktop_transitions_$id", transitions.checkpoint().toString()).apply()
                }
                if (relayProfile != null && savedRelays.value.find { it.id == relayProfile.id } != relayProfile) {
                    if (savedRelays.value.size < 64 || savedRelays.value.any { it.id == relayProfile.id }) {
                        savedRelays.value = savedRelays.value.filterNot { it.id == relayProfile.id } + relayProfile
                        persistRelays()
                    } else error.value = "relay_storage_failed"
                }
                reconnect.forget(id)
                val input = allowInput && (snapshot.optJSONObject("mobile_policy")?.optBoolean("allow_input") ?: negotiatedInput.get())
                computers.value = computers.value.map { if (it.id == id) it.copy(panes = panes, status = "ready", hasConnected = true, failure = null,
                    allowInput = input, pairingApproval = null,
                    runtimeProcess = snapshot.getLong("process_id"), tabs = tabs, windows = windows) else it }
                refreshLanDiscovery()
                events.forEach { SessionNotices.task(context, id, host.name, it, snapshot.getLong("process_id")) }
            }
        }, { failure -> main.post {
            desktopFailed(id, client, failure)
        } }, { approval -> main.post {
            if (desktopClients[id] === client) computers.value = computers.value.map {
                if (it.id == id) it.copy(status = "approval", pairingApproval = approval) else it
            }
        } })
        val replaced = desktopClients.put(id, client)
        scope.launch {
            try {
                // 先交接所有权再异步释放旧连接；旧回调不会覆盖新状态，也不在 UI 线程等待网络。
                replaced?.close()
                if (desktopClients[id] !== client) { client.close(); return@launch }
                val hello = client.connect(allowInput)
                negotiatedInput.set(allowInput && hello.optJSONObject("capabilities")?.optBoolean("input") == true)
                val icon = desktopOsIcon(hello.optJSONObject("host")?.optString("os").orEmpty())
                main.post {
                    if (desktopClients[id] === client) computers.value = computers.value.map {
                        // v2 权限以实时快照为准，图标更新不能覆盖后续权限撤销。
                        if (it.id == id) it.copy(allowInput = if (relayProfile?.version != 2) negotiatedInput.get() else it.allowInput,
                            host = if (icon == "term") it.host else it.host.copy(icon = icon)) else it
                    }
                    if (desktopClients[id] === client && relayProfile != null && icon != "term" && relayProfile.hostOs != icon) {
                        relayProfile.hostOs = icon
                        persistRelays()
                    }
                }
            } catch (cancelled: CancellationException) {
                client.close()
                throw cancelled
            } catch (failure: Exception) {
                client.close()
                main.post {
                    desktopFailed(id, client, classifyDesktopFailure(failure))
                }
            }
        }
        return id
    }
    private fun desktopFailed(id: String, client: DesktopRuntimeClient, failure: DesktopFailureKind, immediate: Boolean = false) {
        if (!desktopClients.remove(id, client)) return // A late old callback cannot remove a recovered client.
        val current = computers.value.find { it.id == id } ?: return
        computers.value = computers.value.map { if (it.id == id) it.copy(status = if (it.hasConnected) "disconnected" else "failed", allowInput = false, failure = failure, pairingApproval = null) else it }
        if (current.hasConnected && current.relayProfile != null && DesktopReconnect.retryable(failure)) reconnect.schedule(id, immediate)
        else error.value = failure.code
        refreshLanDiscovery()
        stopIdleService()
    }
    fun readDesktop(id: String, pane: DesktopPane) {
        if (output.value.target == "$id:${pane.window}:${pane.id}") desktopReader?.refresh()
    }
    suspend fun watchDesktop(id: String, pane: DesktopPane) {
        val client = desktopClients[id] ?: return
        val identity = "$id:${pane.window}:${pane.id}"
        readJob?.cancel()
        val generation = ++readGeneration
        val reader = DesktopReadScheduler()
        desktopReader = reader
        readJob = currentCoroutineContext().job
        output.value = if (output.value.target == identity) output.value else DesktopOutput(identity, loading = true)
        try {
            suspend fun publish(read: DesktopPaneRead) {
                val previous = output.value
                val next = withContext(Dispatchers.IO) {
                    val response = read.response
                    val frame = if (!read.screenChanged && previous.frame != null) previous.frame else response.optJSONObject("screen")?.let {
                        decodeDesktopScreen(it, checkNotNull(terminalColors))
                    }
                    DesktopOutput(identity, response.optString("text"), frame = frame)
                }
                currentCoroutineContext().ensureActive()
                if (generation != readGeneration || desktopClients[id] !== client) throw CancellationException()
                output.value = next
            }
            // Negotiated change stream: no pane.read timer or input-triggered
            // round trip. Older desktops retain the explicit snapshot fallback.
            if (client.streamPane(target(pane).put("lines", 100), ::publish)) return
            reader.run {
                val previous = output.value
                val result = runCatching {
                    withContext(Dispatchers.IO) {
                        val read = client.readPane(target(pane).put("lines", 100))
                        val response = read.response
                        val frame = if (!read.screenChanged && previous.frame != null) previous.frame else response.optJSONObject("screen")?.let {
                            decodeDesktopScreen(it, checkNotNull(terminalColors))
                        }
                        DesktopOutput(identity, response.optString("text"), frame = frame)
                    }
                }
                currentCoroutineContext().ensureActive()
                if (generation != readGeneration || desktopClients[id] !== client) throw CancellationException()
                val next = result.getOrNull() ?: previous.copy(loading = false)
                output.value = next
                if (result.isFailure) {
                    client.resetScreen()
                    error.value = "desktop_read_failed"
                }
                next.frame !== previous.frame || next.text != previous.text
            }
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (_: Exception) {
            if (generation == readGeneration) {
                output.value = output.value.copy(loading = false)
                error.value = "desktop_read_failed"
            }
        } finally {
            if (generation == readGeneration) {
                desktopReader = null
                readJob = null
            }
        }
    }
    fun leaveDesktopPane(clearOutput: Boolean = false) {
        readGeneration++
        readJob?.cancel()
        desktopReader = null
        // 只保留最近一帧供页面重建，读取任务仍立即结束；主动关闭时释放它。
        if (clearOutput) output.value = DesktopOutput()
    }
    fun desktopInput(id: String, pane: DesktopPane): DesktopTerminalInput {
        val client = desktopClients[id]
        return DesktopTerminalInput(
            request = { method, params ->
                checkNotNull(client).request(method, params.put("window_id", pane.window).put("pane_id", pane.id))
            },
            active = { client != null && desktopClients[id] === client &&
                computers.value.any { it.id == id && it.allowInput && it.status == "ready" } },
            onAccepted = { if (output.value.target == "$id:${pane.window}:${pane.id}") readDesktop(id, pane) },
            onRejected = { uncertain -> error.value = if (uncertain) "delivery_unknown" else "input_rejected" },
            dispatch = { method, params ->
                checkNotNull(client).dispatchInput(method, params.put("window_id", pane.window).put("pane_id", pane.id))
            },
        )
    }

    fun desktopTabs(id: String): DesktopTabs {
        val client = desktopClients[id]
        val owner = computers.value.find { it.id == id }
        return DesktopTabs(request = { method, params -> checkNotNull(client).request(method, params) },
            availability = { write, tab ->
                val current = computers.value.find { it.id == id }
                when {
                    client == null || desktopClients[id] !== client || current?.status != "ready" -> "desktop_disconnected"
                    owner == null || owner.runtimeProcess != current.runtimeProcess || owner.connectionGeneration != current.connectionGeneration -> "desktop_session_changed"
                    write && !current.allowInput -> "input_not_authorized"
                    tab != null && current.tabs.none { it.window == tab.window && it.id == tab.id } -> "target_not_found"
                    else -> null
                }
            })
    }

    fun desktopConversation(id: String, pane: DesktopPane, identity: ConversationIdentity): DesktopConversation {
        val client = desktopClients[id]
        val owner = computers.value.find { it.id == id }
        val key = "$id:${owner?.runtimeProcess}:${pane.window}:${pane.id}:${identity.kind}:${identity.session}"
        return DesktopConversation(identity, key, conversationCache,
            request = { method, params -> checkNotNull(client).request(method, params.put("window_id", pane.window).put("pane_id", pane.id)) },
            availability = { write ->
                val current = computers.value.find { it.id == id }
                val active = current?.panes?.find { it.window == pane.window && it.id == pane.id }
                val agent = active?.agent
                when {
                    client == null || desktopClients[id] !== client || current?.status != "ready" -> "desktop_disconnected"
                    owner == null || owner.runtimeProcess != current.runtimeProcess || owner.connectionGeneration != current.connectionGeneration -> "desktop_session_changed"
                    agent?.kind != identity.kind || agent.session != identity.session -> "conversation_identity_changed"
                    write && !current.allowInput -> "input_not_authorized"
                    else -> null
                }
            })
    }

    fun desktopGit(target: DesktopGitTarget): DesktopGit {
        activeGit?.takeIf { it.target == target }?.let { return it }
        val client = desktopClients[target.desktop]
        return DesktopGit(target, scope,
            request = { method, params -> checkNotNull(client).request(method, params) },
            availability = { write ->
                if (client == null || desktopClients[target.desktop] !== client) "git_disconnected"
                else target.availability(computers.value.find { it.id == target.desktop }, write)
            },
            committed = { sent ->
                main.post { if (drafts.value[target.draftKey] == sent) drafts.value = drafts.value - target.draftKey }
            },
        ).also { activeGit = it }
    }
    suspend fun sendDesktop(id: String, pane: DesktopPane, text: String): Boolean {
        if (computers.value.none { it.id == id && it.allowInput && it.status == "ready" }) return false
        return try {
            checkNotNull(desktopClients[id]).request("pane.prompt", target(pane).put("text", text).put("submit", true))
            if (output.value.target == "$id:${pane.window}:${pane.id}") readDesktop(id, pane)
            true
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (_: Exception) {
            error.value = "delivery_unknown"
            false
        }
    }
    private fun target(pane: DesktopPane) = JSONObject().put("window_id", pane.window).put("pane_id", pane.id)
    fun closeAll() {
        discoveryJobs.values.forEach(Job::cancel); discoveryJobs.clear()
        sshConnections.clear()
        activeGit = null
        reconnect.clear()
        resumeChecks.values.forEach(Job::cancel)
        resumeChecks.clear()
        leaveDesktopPane(clearOutput = true)
        pendingTrust.values.forEach { it.complete(false) }; pendingTrust.clear()
        trust.value?.answer?.complete(false); trust.value = null
        live.value.forEach { it.terminal.finishIfRunning() }; live.value = emptyList()
        val clients = desktopClients.values.toList(); desktopClients.clear(); computers.value = emptyList()
        desktopTransitions.clear()
        refreshLanDiscovery()
        drafts.value = emptyMap()
        recentCommands.value = emptyMap()
        scope.launch { clients.forEach { it.close() } }
        stopIdleService()
    }
    fun closeDesktop(id: String) {
        desktopTransitions.remove(id)
        if (activeGit?.target?.desktop == id) activeGit = null
        reconnect.forget(id)
        resumeChecks.remove(id)?.cancel()
        cancelTrust(id)
        if (output.value.target.startsWith("$id:")) leaveDesktopPane(clearOutput = true)
        val client = desktopClients.remove(id)
        computers.value = computers.value.filterNot { it.id == id }
        refreshLanDiscovery()
        drafts.value = drafts.value.filterKeys { !it.startsWith("$id:") }
        recentCommands.value = recentCommands.value.filterKeys { !it.startsWith("$id:") }
        scope.launch { client?.close() }
        stopIdleService()
    }
    private fun stopIdleService() {
        if (live.value.none { it.status in setOf("ready", "connecting") } &&
            computers.value.none { it.status in setOf("ready", "connecting", "approval") }) {
            context.stopService(android.content.Intent(context, SessionService::class.java))
        }
    }
}
