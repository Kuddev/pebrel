package io.github.kuddev.pebrel.mobile.connection

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.Build
import android.os.Handler
import android.os.Looper
import androidx.annotation.RequiresApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import okhttp3.HttpUrl
import java.io.Closeable
import java.net.Inet4Address
import java.util.concurrent.Executor

internal data class PairingComputer(val id: String, val name: String, val address: HttpUrl, val pin: String)
internal data class PairingDiscoveryState(val computers: List<PairingComputer> = emptyList(), val failed: Boolean = false)

/** The pairing form or foreground reconnect owner closes discovery; it never persists credentials. */
internal class PairingDiscovery(context: Context) : Closeable {
    private val manager = context.applicationContext.getSystemService(NsdManager::class.java)
    private val main = Handler(Looper.getMainLooper())
    private val executor = Executor { main.post(it) }
    private val mutableState = MutableStateFlow(PairingDiscoveryState())
    val state = mutableState.asStateFlow()
    private var closed = false
    private var started = false
    private val found = linkedMapOf<String, NsdServiceInfo>()
    private val computers = linkedMapOf<String, PairingComputer>()
    private val pending = linkedMapOf<String, NsdServiceInfo>()
    private val registrations = mutableMapOf<String, Any>()
    private var resolving: String? = null

    private fun publish() {
        mutableState.value = mutableState.value.copy(computers = computers.values.sortedBy { it.name.lowercase() })
    }

    private fun update(id: String, info: NsdServiceInfo) {
        if (closed || id !in found) return
        val parsed = runCatching {
            fun attribute(key: String) = info.attributes[key]?.toString(Charsets.UTF_8).orEmpty()
            require(attribute("version") == "2")
            val pin = attribute("pin")
            require(Regex("sha256/[A-Za-z0-9+/]{43}=").matches(pin))
            require(info.port in 1..65535)
            @Suppress("DEPRECATION")
            val addresses = if (Build.VERSION.SDK_INT >= 34) info.hostAddresses else listOfNotNull(info.host)
            val address = addresses.firstOrNull { it is Inet4Address }
                ?: addresses.firstOrNull { !it.isLinkLocalAddress }
            require(address != null && !address.isAnyLocalAddress && !address.isMulticastAddress)
            val url = HttpUrl.Builder().scheme("https").host(checkNotNull(address.hostAddress)).port(info.port).build()
            val name = attribute("name").trim().take(80).ifEmpty { info.serviceName.take(80) }
            PairingComputer(id, name, url, pin)
        }.getOrNull()
        if (parsed != null) computers[id] = parsed else computers.remove(id)
        publish()
    }

    private val listener = object : NsdManager.DiscoveryListener {
        override fun onDiscoveryStarted(type: String) = Unit
        override fun onDiscoveryStopped(type: String) = Unit
        override fun onStopDiscoveryFailed(type: String, error: Int) = Unit
        override fun onStartDiscoveryFailed(type: String, error: Int) {
            main.post {
                if (!closed) {
                    mutableState.value = mutableState.value.copy(failed = true)
                    runCatching { manager.stopServiceDiscovery(this) }
                    started = false
                }
            }
        }
        override fun onServiceFound(info: NsdServiceInfo) {
            main.post {
                if (closed || info.serviceType.trimEnd('.') != SERVICE_TYPE.trimEnd('.')) return@post
                val id = info.serviceName
                if (id in found || found.size >= 32) return@post
                found[id] = info
                if (Build.VERSION.SDK_INT >= 34) track(id, info)
                else { pending[id] = info; resolveNext() }
            }
        }
        override fun onServiceLost(info: NsdServiceInfo) {
            main.post {
                if (closed) return@post
                val id = info.serviceName
                found.remove(id)
                pending.remove(id)
                computers.remove(id)
                if (Build.VERSION.SDK_INT >= 34) untrack(id)
                publish()
            }
        }
    }

    fun start() {
        check(Looper.myLooper() == Looper.getMainLooper())
        check(!closed && !started)
        runCatching {
            manager.discoverServices(SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, listener)
            started = true
        }.onFailure { mutableState.value = mutableState.value.copy(failed = true) }
    }

    @RequiresApi(34)
    private fun track(id: String, info: NsdServiceInfo) {
        val callback = object : NsdManager.ServiceInfoCallback {
            override fun onServiceUpdated(info: NsdServiceInfo) = update(id, info)
            override fun onServiceLost() { if (!closed) { computers.remove(id); publish() } }
            override fun onServiceInfoCallbackUnregistered() = Unit
            override fun onServiceInfoCallbackRegistrationFailed(error: Int) {
                registrations.remove(id)
                if (!closed) { computers.remove(id); publish() }
            }
        }
        registrations[id] = callback
        runCatching { manager.registerServiceInfoCallback(info, executor, callback) }
            .onFailure { registrations.remove(id) }
    }

    @RequiresApi(34)
    private fun untrack(id: String) {
        (registrations.remove(id) as? NsdManager.ServiceInfoCallback)?.let {
            runCatching { manager.unregisterServiceInfoCallback(it) }
        }
    }

    @Suppress("DEPRECATION")
    private fun resolveNext() {
        if (closed || resolving != null) return
        val next = pending.entries.firstOrNull() ?: return
        val id = next.key
        val info = next.value
        pending.remove(id)
        resolving = id
        val callback = object : NsdManager.ResolveListener {
            override fun onResolveFailed(info: NsdServiceInfo, error: Int) {
                main.post { resolving = null; resolveNext() }
            }
            override fun onServiceResolved(info: NsdServiceInfo) {
                main.post { update(id, info); resolving = null; resolveNext() }
            }
        }
        // Android 8–13 的旧 API 一次只解析一个服务，避免多台电脑互相触发 ALREADY_ACTIVE。
        runCatching { manager.resolveService(info, callback) }.onFailure { resolving = null; resolveNext() }
    }

    override fun close() {
        check(Looper.myLooper() == Looper.getMainLooper())
        if (closed) return
        closed = true
        if (started) runCatching { manager.stopServiceDiscovery(listener) }
        if (Build.VERSION.SDK_INT >= 34) registrations.keys.toList().forEach(::untrack)
        pending.clear()
        found.clear()
        computers.clear()
    }

    companion object { private const val SERVICE_TYPE = "_pebrel-pair._tcp." }
}
