package org.virtueinitiative.virtue

import android.app.AlarmManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel

// Lightweight foreground service that keeps the process alive and holds the notification.
// Screen capture is performed by VirtueAccessibilityService.takeScreenshot().
class ScreenshotService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        startForeground(NOTIFICATION_ID, buildNotification("Monitoring active"))

        val initError = NativeBridge.ensureInitialized(this)
        if (initError != null) {
            Log.e(TAG, "Core init failed: $initError")
            updateNotification("Core init failed")
            stopSelf()
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            stopSelf()
            return START_NOT_STICKY
        }
        return START_STICKY
    }

    override fun onDestroy() {
        scope.cancel()
        super.onDestroy()
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        val restartIntent = Intent(applicationContext, ScreenshotService::class.java)
        val pendingIntent = PendingIntent.getService(
            applicationContext,
            1995,
            restartIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        val alarmManager = getSystemService(Context.ALARM_SERVICE) as AlarmManager
        val triggerAtMillis = System.currentTimeMillis() + 5_000L

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && !alarmManager.canScheduleExactAlarms()) {
            alarmManager.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, triggerAtMillis + 10_000L, pendingIntent)
        } else {
            runCatching {
                alarmManager.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, triggerAtMillis, pendingIntent)
            }.onFailure {
                alarmManager.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, triggerAtMillis + 10_000L, pendingIntent)
            }
        }
        super.onTaskRemoved(rootIntent)
    }

    fun updateNotification(text: String) {
        val manager = getSystemService(NotificationManager::class.java)
        manager.notify(NOTIFICATION_ID, buildNotification(text))
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val channel = NotificationChannel(CHANNEL_ID, "Virtue monitoring", NotificationManager.IMPORTANCE_LOW)
        channel.description = "Background screenshot accountability monitoring"
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    private fun buildNotification(text: String): Notification {
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_menu_camera)
            .setContentTitle("Virtue")
            .setContentText(text)
            .setOngoing(true)
            .build()
    }

    companion object {
        private const val TAG = "ScreenshotService"
        private const val ACTION_STOP = "org.virtueinitiative.virtue.STOP"
        private const val CHANNEL_ID = "virtue_monitoring"
        private const val NOTIFICATION_ID = 1001

        fun start(context: Context): String? {
            val intent = Intent(context, ScreenshotService::class.java)
            return runCatching {
                ContextCompat.startForegroundService(context, intent)
            }.exceptionOrNull()?.message
        }

        fun stop(context: Context) {
            val intent = Intent(context, ScreenshotService::class.java).apply {
                action = ACTION_STOP
            }
            context.startService(intent)
        }

        // Called by the Rust JNI layer — delegates to VirtueAccessibilityService
        @JvmStatic
        fun captureStatusForDaemon(): Int = VirtueAccessibilityService.captureStatusForDaemon()

        @JvmStatic
        fun capturePngForDaemon(): ByteArray? = VirtueAccessibilityService.capturePngForDaemon()
    }
}
