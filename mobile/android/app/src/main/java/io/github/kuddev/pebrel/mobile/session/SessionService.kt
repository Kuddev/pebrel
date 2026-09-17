package io.github.kuddev.pebrel.mobile.session

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.IBinder
import io.github.kuddev.pebrel.mobile.MainActivity
import io.github.kuddev.pebrel.mobile.PebrelApplication
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.DesktopPane

class SessionService : Service() {
    override fun onDestroy() {
        (application as PebrelApplication).sessions.backgroundActive.value = false
        super.onDestroy()
    }
    override fun onBind(intent: Intent?): IBinder? = null
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == "STOP") {
            (application as PebrelApplication).sessions.closeAll()
            stopForeground(STOP_FOREGROUND_REMOVE); stopSelf(); return START_NOT_STICKY
        }
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(NotificationChannel("sessions", getString(R.string.background_title), NotificationManager.IMPORTANCE_LOW))
        val open = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        val stop = PendingIntent.getService(this, 1, Intent(this, SessionService::class.java).setAction("STOP"), PendingIntent.FLAG_IMMUTABLE)
        startForeground(1, Notification.Builder(this, "sessions").setSmallIcon(R.drawable.ic_pebrel)
            .setContentTitle(getString(R.string.app_name)).setContentText(getString(R.string.background_active))
            .setContentIntent(open).setOngoing(true).addAction(Notification.Action.Builder(null, getString(R.string.stop_sessions), stop).build()).build())
        (application as PebrelApplication).sessions.backgroundActive.value = true
        return START_NOT_STICKY
    }
}

object SessionNotices {
    fun task(context: Context, desktop: String, host: String, pane: DesktopPane) {
        if (android.os.Build.VERSION.SDK_INT >= 33 && context.checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) return
        val manager = context.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(NotificationChannel("tasks", context.getString(R.string.task_notifications), NotificationManager.IMPORTANCE_DEFAULT))
        val key = "$desktop:${pane.window}:${pane.id}"
        val intent = Intent(context, MainActivity::class.java).setAction("OPEN_TASK").setData(android.net.Uri.parse("pebrel://task/$key"))
            .putExtra("desktop", desktop).putExtra("window", pane.window).putExtra("pane", pane.id)
        val pending = PendingIntent.getActivity(context, 0, intent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        manager.notify(key, 2, Notification.Builder(context, "tasks").setSmallIcon(R.drawable.ic_pebrel)
            .setContentTitle(host).setContentText(context.getString(R.string.task_updated)).setContentIntent(pending)
            .setVisibility(Notification.VISIBILITY_PRIVATE).setAutoCancel(true).build())
    }
}
