package io.github.kuddev.pebrel.mobile.session

import android.content.Context
import android.os.Handler
import android.os.Looper
import io.github.kuddev.pebrel.terminal.LocalPtyTransport
import io.github.kuddev.pebrel.terminal.SessionTransport
import io.github.kuddev.pebrel.terminal.TerminalSession
import io.github.kuddev.pebrel.terminal.TerminalCallbacks
import io.github.kuddev.pebrel.mobile.connection.*
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit

data class LocalSession(
    val id: String, val title: String, val source: String, val terminal: TerminalSession,
    val status: String = "connecting", val host: HostProfile? = null,
    val stage: SshStage = SshStage.NETWORK, val failure: SshFailureKind? = null,
)
data class DesktopWorkspace(
    val id: String,
    val host: HostProfile,
    val windows: List<DesktopWindow> = emptyList(),
    val status: String = "connecting",
    val allowInput: Boolean = false,
    val canSendKeys: Boolean = false,
    val canCreateTabs: Boolean = false,
    val canCloseTabs: Boolean = false,
    val transport: String = "SSH",
    val processId: Long = -1,
    val revision: Long = -1,
) {
    val tabs: List<DesktopTab> get() = windows.flatMap(DesktopWindow::tabs)
    val panes: List<DesktopPane> get() = tabs.flatMap(DesktopTab::panes)
    fun preferredWindow(): DesktopWindow? = windows.find(DesktopWindow::focused) ?: windows.firstOrNull()
}
enum class DesktopTabCloseOutcome { CLOSED, CONFIRMATION_REQUIRED, FAILED }
data class TrustRequest(val ownerId: String, val host: HostProfile, val fingerprint: String, val answer: CompletableFuture<Boolean>)
data class DesktopOutput(val target: String = "", val text: String = "", val loading: Boolean = false)

/** Application owns sessions; activities only attach views. Metadata never updates per cell. */
class SessionRepository(private val context: Context) {
    val display = DisplayPreferences(context)
    private val main = Handler(Looper.getMainLooper())
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val preferences = context.getSharedPreferences("pebrel_mobile", Context.MODE_PRIVATE)
    private val credentialStore = HostCredentialStore(context)
    private val live = MutableStateFlow<List<LocalSession>>(emptyList())
    val sessions = live.asStateFlow()
    private val computers = MutableStateFlow<List<DesktopWorkspace>>(emptyList())
    val desktops = computers.asStateFlow()
    private val relayStore = RelayProfileStore(context)
    private val relayWrites = Mutex()
    private val savedRelays = MutableStateFlow<List<RelayProfile>>(emptyList())
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
    private val pendingTrust = java.util.concurrent.ConcurrentHashMap<String, CompletableFuture<Boolean>>()
    private var readJob: Job? = null
    private var readGeneration = 0L
    private val desktopClients = java.util.concurrent.ConcurrentHashMap<String, DesktopRuntimeClient>()
    private val desktopTransitions = java.util.concurrent.ConcurrentHashMap<String, DesktopTransitions>()
    private val desktopMutationLocks = java.util.concurrent.ConcurrentHashMap<String, Mutex>()
    private val desktopInputLocks = java.util.concurrent.ConcurrentHashMap<String, Mutex>()
    private var renderOwner: String? = null
    private var renderToken: Any? = null
    private var redraw: (() -> Unit)? = null
    val drafts = MutableStateFlow<Map<String, String>>(emptyMap())
    fun setDraft(id: String, text: String) { drafts.value = drafts.value + (id to text) }
    fun acknowledgeDraft(id: String, sent: String) {
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
                    withContext(Dispatchers.Main) {
                        savedRelays.value = (savedRelays.value + restored).distinctBy { it.id }
                    }
                }.onFailure { error.value = "relay_storage_failed" }
            }
        }
    }

    private fun loadHosts(): List<HostProfile> = runCatching {
        val rows = JSONArray(preferences.getString("hosts", "[]"))
        (0 until rows.length()).map { i -> rows.getJSONObject(i).let {
            HostProfile(it.getString("id"), it.getString("name"), it.getString("address"), it.getInt("port"), it.getString("user"),
                it.optString("fingerprint"), it.optString("icon", "term"), it.optString("group", "development"))
        } }
    }.getOrDefault(emptyList())

    fun saveHost(host: HostProfile) {
        val previous = savedHosts.value.find { it.id == host.id }
        val fingerprint = if (previous?.address == host.address && previous.port == host.port) previous.fingerprint else ""
        savedHosts.value = savedHosts.value.filterNot { it.id == host.id } + host.copy(fingerprint = fingerprint)
        persistHosts()
    }

    /** Credentials are scoped to both the saved identity and the login endpoint. */
    private fun credentialKey(host: HostProfile): String {
        val identity = "${host.id}\u0000${host.address}\u0000${host.port}\u0000${host.user}"
        return java.security.MessageDigest.getInstance("SHA-256").digest(identity.toByteArray())
            .joinToString("") { "%02x".format(it.toInt() and 255) }
    }

    fun hasSavedPassword(host: HostProfile): Boolean = savedCredentialIds.value.contains(credentialKey(host))

    /** Completes only after encrypted credentials and host metadata reach storage. */
    suspend fun saveHostWithCredentials(host: HostProfile, password: CharArray?, rememberPassword: Boolean): Boolean =
        withContext(Dispatchers.IO) {
            hostWrites.withLock {
                credentialWrites.withLock {
                    try {
                        val previous = savedHosts.value.find { it.id == host.id }
                        val key = credentialKey(host)
                        if (rememberPassword && password?.isNotEmpty() == true) credentialStore.save(key, password)
                        else if (!rememberPassword) credentialStore.clear(key)
                        if (previous != null && credentialKey(previous) != key) credentialStore.clear(credentialKey(previous))
                        val fingerprint = if (previous?.address == host.address && previous.port == host.port) previous.fingerprint else ""
                        val next = savedHosts.value.filterNot { it.id == host.id } + host.copy(fingerprint = fingerprint)
                        check(writeHostMetadata(next))
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
            try { credentialStore.load(credentialKey(host)) }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (_: Exception) { error.value = "credential_load_failed"; null }
        }
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
        scope.launch { clearHostPassword(host) }
    }

    private fun writeHostMetadata(hosts: List<HostProfile>): Boolean {
        val rows = JSONArray()
        hosts.forEach { h -> rows.put(JSONObject().put("id", h.id).put("name", h.name).put("address", h.address)
            .put("port", h.port).put("user", h.user).put("fingerprint", h.fingerprint).put("icon", h.icon).put("group", h.group)) }
        return preferences.edit().putString("hosts", rows.toString()).commit()
    }

    private fun persistHosts() {
        scope.launch {
            hostWrites.withLock {
                if (!writeHostMetadata(savedHosts.value)) error.value = "host_save_failed"
            }
        }
    }
    private fun verify(ownerId: String, host: HostProfile, fingerprint: String): Boolean {
        val answer = CompletableFuture<Boolean>()
        pendingTrust[ownerId] = answer
        main.post {
            val active = live.value.any { it.id == ownerId && it.status == "connecting" } ||
                computers.value.any { it.id == ownerId && it.status == "connecting" }
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
    private var terminalColors: IntArray? = null
    fun setTerminalColors(value: IntArray) {
        terminalColors = value.copyOf()
        live.value.forEach { it.terminal.colors(value) }
    }
    fun local(): String = addTerminal("Term", "Local", LocalPtyTransport(context.filesDir.absolutePath))
    fun ssh(host: HostProfile, password: CharArray): String {
        val id = UUID.randomUUID().toString()
        val connection = SshConnection(host, password, { h, fingerprint -> verify(id, h, fingerprint) }) { stage ->
            main.post { update(id) { if (it.status == "connecting") it.copy(stage = stage) else it } }
        }
        return addTerminal(host.name, "SSH", SshTerminalTransport(connection), id, host)
    }

    private fun addTerminal(title: String, source: String, transport: SessionTransport,
                            id: String = UUID.randomUUID().toString(), host: HostProfile? = null): String {
        val callbacks = object : TerminalCallbacks() {
            override fun onTextChanged(session: TerminalSession) { if (renderOwner == id) redraw?.invoke() }
            override fun onTitleChanged(session: TerminalSession) { update(id) { it.copy(title = session.title?.take(80) ?: it.title) } }
            override fun onTransportReady(session: TerminalSession) { update(id) { it.copy(status = "ready") } }
            override fun onSessionFinished(session: TerminalSession) {
                cancelTrust(id)
                update(id) { it.copy(status = if (session.failure == null) "ended" else "failed",
                    failure = session.failureCause?.let(::classifySshFailure)) }
                stopIdleService()
            }
            override fun onInputRejected(session: TerminalSession) { error.value = "input_rejected" }

        }
        val terminal = TerminalSession(transport, callbacks)
        live.value = live.value + LocalSession(id, title, source, terminal, host = host)
        terminal.start()
        terminalColors?.let { terminal.colors(it) }
        return id
    }
    private fun update(id: String, change: (LocalSession) -> LocalSession) { live.value = live.value.map { if (it.id == id) change(it) else it } }
    fun closeTerminal(id: String) {
        cancelTrust(id)
        live.value.find { it.id == id }?.terminal?.finishIfRunning()
        live.value = live.value.filterNot { it.id == id }
        drafts.value = drafts.value - id
        stopIdleService()
    }
    fun importRelay(text: String): String? {
        return try {
            val profile = RelayProfile.parse(text)
            check(savedRelays.value.size < 64 || savedRelays.value.any { it.id == profile.id })
            savedRelays.value = savedRelays.value.filterNot { it.id == profile.id } + profile
            persistRelays()
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
        persistRelays()
    }
    fun connectRelay(profile: RelayProfile): String {
        computers.value.find { it.host.id == profile.id && it.status in setOf("ready", "connecting") }?.let { return it.id }
        computers.value.filter { it.host.id == profile.id }.forEach { closeDesktop(it.id) }
        val host = HostProfile(profile.id, profile.name, profile.url, 443, "")
        return addDesktop(host, true, RelayTransport(profile), "Relay")
    }
    fun connectDesktop(host: HostProfile, password: CharArray, allowInput: Boolean): String {
        val id = UUID.randomUUID().toString()
        return addDesktop(host, allowInput, SshDesktopTransport(SshConnection(host, password,
            { h, fingerprint -> verify(id, h, fingerprint) })), "SSH", id)
    }

    private fun addDesktop(host: HostProfile, allowInput: Boolean, transport: DesktopTransport, source: String,
                           id: String = UUID.randomUUID().toString()): String {
        computers.value = computers.value + DesktopWorkspace(id, host, allowInput = false, transport = source)
        val transitions = DesktopTransitions()
        desktopTransitions[id] = transitions
        val client = DesktopRuntimeClient(transport, { snapshot ->
            main.post { applyDesktopSnapshot(id, host, transitions, snapshot) }
        }, { main.post {
            desktopClients.remove(id)
            computers.value = computers.value.map { if (it.id == id) it.copy(status = "disconnected") else it }
            stopIdleService()
        } })
        desktopClients[id] = client
        scope.launch {
            try {
                val hello = client.connect(allowInput)
                val capabilities = desktopBridgeCapabilities(allowInput, hello.optJSONObject("capabilities"))
                main.post {
                    computers.value = computers.value.map {
                        if (it.id == id) it.copy(
                            allowInput = capabilities.allowInput,
                            canSendKeys = capabilities.canSendKeys,
                            canCreateTabs = capabilities.canCreateTabs,
                            canCloseTabs = capabilities.canCloseTabs,
                        ) else it
                    }
                }
            } catch (_: Exception) {
                desktopClients.remove(id, client)
                client.close()
                main.post {
                    if (computers.value.any { it.id == id }) {
                        computers.value = computers.value.map { if (it.id == id) it.copy(status = "failed") else it }
                        error.value = if (source == "Relay") "relay_connection_failed" else "desktop_connection_failed"
                    }
                    stopIdleService()
                }
            }
        }
        return id
    }

    private fun applyDesktopSnapshot(
        id: String,
        host: HostProfile,
        transitions: DesktopTransitions,
        snapshot: JSONObject,
    ) {
        val current = computers.value.find { it.id == id } ?: return
        if (!shouldApplyDesktopSnapshot(current.processId, current.revision, snapshot)) return
        val windows = parseDesktopWindows(snapshot)
        val events = transitions.observe(snapshot)
        val processId = snapshot.optLong("process_id", -1L)
        val revision = snapshot.optLong("revision", 0L)
        computers.value = computers.value.map {
            if (it.id == id) it.copy(
                windows = windows,
                status = "ready",
                processId = processId,
                revision = revision,
            ) else it
        }
        if (computers.value.any { it.id == id }) {
            events.forEach { SessionNotices.task(context, id, host.name, it) }
        }
    }

    private suspend fun applyMutationSnapshot(id: String, snapshot: JSONObject?) {
        if (snapshot == null) return
        withContext(Dispatchers.Main) {
            val workspace = computers.value.find { it.id == id } ?: return@withContext
            val transitions = desktopTransitions[id] ?: return@withContext
            applyDesktopSnapshot(id, workspace.host, transitions, snapshot)
        }
    }

    suspend fun createDesktopTab(id: String, windowId: Long): DesktopPane? {
        val lock = desktopMutationLocks.computeIfAbsent(id) { Mutex() }
        return lock.withLock {
            val workspace = computers.value.find {
                it.id == id && it.status == "ready" && it.canCreateTabs && it.windows.any { window -> window.id == windowId }
            } ?: return@withLock null
            try {
                val result = checkNotNull(desktopClients[id]).request(
                    "tab.new",
                    JSONObject().put("window_id", windowId),
                )
                applyMutationSnapshot(id, result.optJSONObject("snapshot"))
                val action = result.optJSONObject("action")
                val createdWindow = action?.optLong("window_id", windowId) ?: windowId
                val createdPane = action?.optLong("pane_id", -1L) ?: -1L
                computers.value.find { it.id == id }?.panes?.find {
                    it.window == createdWindow && it.id == createdPane
                }
            } catch (timeout: TimeoutCancellationException) {
                error.value = "delivery_unknown"
                null
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: DesktopRpcException) {
                error.value = if (failure.code == "input_not_authorized") {
                    "desktop_tab_control_unavailable"
                } else {
                    "desktop_tab_create_failed"
                }
                null
            } catch (_: Exception) {
                error.value = "delivery_unknown"
                null
            }
        }
    }

    suspend fun closeDesktopTab(
        id: String,
        tab: DesktopTab,
        confirmed: Boolean,
    ): DesktopTabCloseOutcome {
        val guardPaneId = tab.closeGuardPaneId ?: return DesktopTabCloseOutcome.FAILED
        val lock = desktopMutationLocks.computeIfAbsent(id) { Mutex() }
        return lock.withLock {
            val workspace = computers.value.find { it.id == id && it.status == "ready" && it.canCloseTabs }
                ?: return@withLock DesktopTabCloseOutcome.FAILED
            val current = workspace.tabs.find {
                it.window == tab.window && it.index == tab.index && it.closeGuardPaneId == guardPaneId
            }
            if (current == null) {
                error.value = "desktop_tab_changed"
                return@withLock DesktopTabCloseOutcome.FAILED
            }
            try {
                val params = JSONObject()
                    .put("window_id", current.window)
                    .put("tab_index", current.index)
                    .put("expected_pane_id", guardPaneId)
                    .put("confirmed", confirmed)
                val result = checkNotNull(desktopClients[id]).request("tab.close", params)
                applyMutationSnapshot(id, result.optJSONObject("snapshot"))
                DesktopTabCloseOutcome.CLOSED
            } catch (timeout: TimeoutCancellationException) {
                error.value = "delivery_unknown"
                DesktopTabCloseOutcome.FAILED
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: DesktopRpcException) {
                when (failure.code) {
                    "confirmation_required" -> DesktopTabCloseOutcome.CONFIRMATION_REQUIRED
                    "stale_target", "target_not_found" -> {
                        error.value = "desktop_tab_changed"
                        DesktopTabCloseOutcome.FAILED
                    }
                    "input_not_authorized" -> {
                        error.value = "desktop_tab_control_unavailable"
                        DesktopTabCloseOutcome.FAILED
                    }
                    else -> {
                        error.value = "desktop_tab_close_failed"
                        DesktopTabCloseOutcome.FAILED
                    }
                }
            } catch (_: Exception) {
                error.value = "delivery_unknown"
                DesktopTabCloseOutcome.FAILED
            }
        }
    }
    fun readDesktop(id: String, pane: DesktopPane) {
        val identity = "$id:${pane.window}:${pane.id}"
        if (readJob?.isActive == true && output.value.target == identity) return
        readJob?.cancel()
        val generation = ++readGeneration
        val previousText = if (output.value.target == identity) output.value.text else ""
        output.value = DesktopOutput(identity, previousText, loading = true)
        readJob = scope.launch {
            val result = runCatching { checkNotNull(desktopClients[id]).request("pane.read", target(pane).put("lines", 120)) }
            if (!isActive) return@launch
            main.post {
                if (generation == readGeneration && output.value.target == identity) {
                    output.value = DesktopOutput(identity, result.getOrNull()?.optString("text") ?: "")
                    if (result.isFailure) error.value = "desktop_read_failed"
                }
            }
        }
    }
    fun leaveDesktopPane() {
        readGeneration++
        readJob?.cancel()
        output.value = DesktopOutput()
    }
    suspend fun sendDesktop(id: String, pane: DesktopPane, text: String): Boolean =
        sendDesktopInput(id, pane, "pane.prompt", { it.allowInput }) {
            put("text", text).put("submit", true)
        }

    suspend fun sendDesktopKey(id: String, pane: DesktopPane, label: String): Boolean {
        val key = desktopControlKey(label) ?: return false
        return sendDesktopInput(id, pane, "pane.send_key", { it.allowInput && it.canSendKeys }) {
            put("key", key.key)
            put("modifiers", JSONObject().put("control", key.control))
            put("repeat", 1)
        }
    }

    /** Prompt and key requests share one ordered lane; uncertain input is never retried. */
    private suspend fun sendDesktopInput(
        id: String,
        pane: DesktopPane,
        method: String,
        allowed: (DesktopWorkspace) -> Boolean,
        configure: JSONObject.() -> Unit,
    ): Boolean {
        val lock = desktopInputLocks.computeIfAbsent(id) { Mutex() }
        return lock.withLock {
            val workspace = computers.value.find { it.id == id && it.status == "ready" }
                ?: return@withLock false
            if (!allowed(workspace)) return@withLock false
            try {
                checkNotNull(desktopClients[id]).request(method, target(pane).apply(configure))
                if (output.value.target == "$id:${pane.window}:${pane.id}") readDesktop(id, pane)
                true
            } catch (_: TimeoutCancellationException) {
                error.value = "delivery_unknown"
                false
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: DesktopRpcException) {
                error.value = desktopInputFailure(failure.code)
                false
            } catch (_: Exception) {
                error.value = "delivery_unknown"
                false
            }
        }
    }
    private fun target(pane: DesktopPane) = JSONObject().put("window_id", pane.window).put("pane_id", pane.id)
    fun closeAll() {
        leaveDesktopPane()
        pendingTrust.values.forEach { it.complete(false) }; pendingTrust.clear()
        trust.value?.answer?.complete(false); trust.value = null
        live.value.forEach { it.terminal.finishIfRunning() }; live.value = emptyList()
        val clients = desktopClients.values.toList(); desktopClients.clear(); computers.value = emptyList()
        desktopTransitions.clear(); desktopMutationLocks.clear(); desktopInputLocks.clear()
        drafts.value = emptyMap()
        scope.launch { clients.forEach { it.close() } }
        stopIdleService()
    }
    fun closeDesktop(id: String) {
        cancelTrust(id)
        leaveDesktopPane()
        val client = desktopClients.remove(id)
        desktopTransitions.remove(id)
        desktopMutationLocks.remove(id)
        desktopInputLocks.remove(id)
        computers.value = computers.value.filterNot { it.id == id }
        drafts.value = drafts.value.filterKeys { !it.startsWith("$id:") }
        scope.launch { client?.close() }
        stopIdleService()
    }
    private fun stopIdleService() {
        if (live.value.none { it.status in setOf("ready", "connecting") } &&
            computers.value.none { it.status in setOf("ready", "connecting") }) {
            context.stopService(android.content.Intent(context, SessionService::class.java))
        }
    }
}
