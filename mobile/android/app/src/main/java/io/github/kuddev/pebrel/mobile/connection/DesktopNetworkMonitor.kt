package io.github.kuddev.pebrel.mobile.connection

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.os.Handler
import android.os.Looper

/** Only observes the foreground default route; callbacks share the repository's UI owner. */
internal class DesktopNetworkMonitor(context: Context, private val changed: () -> Unit) {
    private val manager = context.getSystemService(ConnectivityManager::class.java)
    private val main = Handler(Looper.getMainLooper())
    private var registered = false
    private var seeded = false
    private var current: Network? = null
    private val callback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) {
            if (!registered) return
            val different = seeded && current != network
            current = network
            seeded = true
            if (different) changed()
        }

        override fun onLost(network: Network) {
            if (registered && current == network) current = null
        }
    }

    fun start() {
        if (registered) return
        val active = manager.activeNetwork
        val different = seeded && active != null && current != active
        current = active
        seeded = true
        registered = true
        manager.registerDefaultNetworkCallback(callback, main)
        // 后台期间换网不会给未注册的监听器补事件；恢复时也要比较真实路由。
        if (different) changed()
    }

    fun stop() {
        if (!registered) return
        registered = false
        manager.unregisterNetworkCallback(callback)
    }
}
