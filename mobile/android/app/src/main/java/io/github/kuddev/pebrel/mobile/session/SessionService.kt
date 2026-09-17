package io.github.kuddev.pebrel.mobile.session

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import io.github.kuddev.pebrel.mobile.MainActivity
import io.github.kuddev.pebrel.mobile.PebrelApplication
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.DesktopPane
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.launch

/** Keeps live terminal transports alive after the activity leaves the foreground. */
class SessionService : Service() {
    private val serviceScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private lateinit var repository: SessionRepository
    private var foregroundStarted = false
    private var liveSessionCount = 0
    private var wakeLock: PowerManager.WakeLock? = null
    private var startCommandReceived = false
    private var idleStopPending = false

    override fun onCreate() {
        super.onCreate()
        repository = (application as PebrelApplication).sessions
        createSessionChannel()
        wakeLock = (getSystemService(Context.POWER_SERVICE) as PowerManager)
            .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "${packageName}:sessions")
            .apply { setReferenceCounted(false) }
        // A startForegroundService call has a short deadline. Register the service
        // notification before the StateFlow collector can observe its initial empty state.
        ensureForeground()

        serviceScope.launch {
            combine(repository.sessions, repository.desktops) { sessions, desktops ->
                sessions.count { it.status in LIVE_STATES } + desktops.count { it.status in LIVE_STATES }
            }.distinctUntilChanged().collect { count ->
                onLiveSessionCountChanged(count)
            }
        }
    }

    override fun onDestroy() {
        releaseWakeLock()
        repository.backgroundActive.value = false
        serviceScope.cancel()
        foregroundStarted = false
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        startCommandReceived = true
        when (intent?.action) {
            ACTION_EXIT, LEGACY_STOP_ACTION -> {
                repository.closeAll()
                releaseWakeLock()
                repository.backgroundActive.value = false
                removeForeground()
                stopSelfResult(startId)
            }
            ACTION_TOGGLE_WAKE_LOCK -> {
                liveSessionCount = currentLiveSessionCount()
                ensureForeground()
                if (liveSessionCount == 0) stopForNoSessions() else toggleWakeLock()
            }
            else -> {
                liveSessionCount = currentLiveSessionCount()
                ensureForeground()
                if (liveSessionCount > 0) {
                    repository.backgroundActive.value = true
                    publishNotification()
                    idleStopPending = false
                } else {
                    stopForNoSessions()
                }
            }
        }
        return START_NOT_STICKY
    }

    private fun onLiveSessionCountChanged(count: Int) {
        liveSessionCount = count
        if (count == 0) {
            releaseWakeLock()
            repository.backgroundActive.value = false
            if (startCommandReceived) stopForNoSessions() else idleStopPending = true
            return
        }
        idleStopPending = false
        repository.backgroundActive.value = true
        ensureForeground()
        publishNotification()
    }

    private fun stopForNoSessions() {
        if (!startCommandReceived && !idleStopPending) return
        idleStopPending = false
        releaseWakeLock()
        repository.backgroundActive.value = false
        removeForeground()
        stopSelf()
    }

    private fun currentLiveSessionCount(): Int =
        repository.sessions.value.count { it.status in LIVE_STATES } +
            repository.desktops.value.count { it.status in LIVE_STATES }

    private fun ensureForeground() {
        if (foregroundStarted) return
        startForeground(NOTIFICATION_ID, buildNotification())
        foregroundStarted = true
    }

    private fun removeForeground() {
        if (!foregroundStarted) return
        stopForeground(STOP_FOREGROUND_REMOVE)
        foregroundStarted = false
    }

    private fun toggleWakeLock() {
        if (liveSessionCount == 0) {
            releaseWakeLock()
            return
        }
        val lock = wakeLock ?: return
        if (lock.isHeld) {
            releaseWakeLock()
        } else {
            runCatching { lock.acquire() }
        }
        publishNotification()
    }

    private fun releaseWakeLock() {
        wakeLock?.let { lock ->
            if (lock.isHeld) runCatching { lock.release() }
        }
    }

    private fun publishNotification() {
        if (!foregroundStarted || liveSessionCount == 0) return
        getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, buildNotification())
    }

    private fun buildNotification(): Notification {
        val open = PendingIntent.getActivity(
            this,
            REQUEST_OPEN,
            Intent(this, MainActivity::class.java).addFlags(
                Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP,
            ),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val exit = PendingIntent.getService(
            this,
            REQUEST_EXIT,
            Intent(this, SessionService::class.java).setAction(ACTION_EXIT),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val toggleWakeLock = PendingIntent.getService(
            this,
            REQUEST_WAKE_LOCK,
            Intent(this, SessionService::class.java).setAction(ACTION_TOGGLE_WAKE_LOCK),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val wakeLockHeld = wakeLock?.isHeld == true
        return Notification.Builder(this, SESSION_CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(getString(R.string.app_name))
            .setContentText(resources.getQuantityString(R.plurals.background_live_sessions, liveSessionCount, liveSessionCount))
            .setSubText(if (wakeLockHeld) getString(R.string.notification_wakelock_active) else null)
            .setContentIntent(open)
            .setCategory(Notification.CATEGORY_SERVICE)
            .setVisibility(Notification.VISIBILITY_PUBLIC)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setShowWhen(false)
            .addAction(Notification.Action.Builder(null, getString(R.string.notification_exit), exit).build())
            .addAction(
                Notification.Action.Builder(
                    null,
                    getString(if (wakeLockHeld) R.string.notification_release_wakelock else R.string.notification_acquire_wakelock),
                    toggleWakeLock,
                ).build(),
            )
            .build()
    }

    private fun createSessionChannel() {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(
                SESSION_CHANNEL_ID,
                getString(R.string.background_title),
                NotificationManager.IMPORTANCE_LOW,
            ),
        )
    }

    companion object {
        const val ACTION_EXIT = "io.github.kuddev.pebrel.mobile.session.EXIT"
        const val ACTION_TOGGLE_WAKE_LOCK = "io.github.kuddev.pebrel.mobile.session.TOGGLE_WAKE_LOCK"
        const val ACTION_START = "io.github.kuddev.pebrel.mobile.session.START"

        private const val LEGACY_STOP_ACTION = "STOP"
        private const val SESSION_CHANNEL_ID = "sessions"
        private const val NOTIFICATION_ID = 1
        private const val REQUEST_OPEN = 10
        private const val REQUEST_EXIT = 11
        private const val REQUEST_WAKE_LOCK = 12
        private val LIVE_STATES = setOf("connecting", "ready")

        /** Starts the service from an activity or another user initiated app action. */
        fun start(context: Context) {
            val appContext = context.applicationContext
            val intent = Intent(appContext, SessionService::class.java).setAction(ACTION_START)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) appContext.startForegroundService(intent)
            else appContext.startService(intent)
        }

        /** Idempotent at the Android service boundary; safe to call for each new live session. */
        fun ensureStarted(context: Context) = start(context)

        fun stop(context: Context) {
            context.applicationContext.stopService(Intent(context, SessionService::class.java))
        }

        fun toggleWakeLock(context: Context) {
            context.applicationContext.startService(
                Intent(context, SessionService::class.java).setAction(ACTION_TOGGLE_WAKE_LOCK),
            )
        }

        fun exit(context: Context) {
            context.applicationContext.startService(
                Intent(context, SessionService::class.java).setAction(ACTION_EXIT),
            )
        }
    }
}

object SessionNotices {
    fun task(context: Context, desktop: String, host: String, pane: DesktopPane) {
        if (Build.VERSION.SDK_INT >= 33 && context.checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) return
        val manager = context.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(NotificationChannel("tasks", context.getString(R.string.task_notifications), NotificationManager.IMPORTANCE_DEFAULT))
        val key = "$desktop:${pane.window}:${pane.id}"
        val intent = Intent(context, MainActivity::class.java).setAction("OPEN_TASK").setData(android.net.Uri.parse("pebrel://task/$key"))
            .putExtra("desktop", desktop).putExtra("window", pane.window).putExtra("pane", pane.id)
        val pending = PendingIntent.getActivity(context, 0, intent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        manager.notify(key, 2, Notification.Builder(context, "tasks").setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(host).setContentText(context.getString(R.string.task_updated)).setContentIntent(pending)
            .setVisibility(Notification.VISIBILITY_PRIVATE).setAutoCancel(true).build())
    }
}
