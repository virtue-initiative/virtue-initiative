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
        instance = this
        createNotificationChannel()
        // Android can still refuse a foreground-service start (e.g. a
        // background start with no exemption in effect), and uncaught that
        // throw crashes the whole process ("Virtue keeps closing", #709).
        // Monitoring runs in VirtueAccessibilityService either way, so just
        // go without the notification until the next start attempt.
        try {
            startForeground(NOTIFICATION_ID, buildNotification())
        } catch (e: IllegalStateException) {
            Log.w(TAG, "Foreground service start refused", e)
            stopSelf()
            return
        }

        val initError = NativeBridge.ensureInitialized(this)
        if (initError != null) {
            Log.e(TAG, "Core init failed: $initError")
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
        if (instance === this) instance = null
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

    /** Re-posts the notification for the current Accessibility state (CLIENT-030). */
    fun updateNotification() {
        getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, buildNotification())
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val channel = NotificationChannel(CHANNEL_ID, "Virtue monitoring", NotificationManager.IMPORTANCE_LOW)
        channel.description = "Background screenshot accountability monitoring"
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    private fun buildNotification(): Notification {
        val notice = MonitoringNotice.text(VirtueAccessibilityService.isConnected())
        val target = if (notice.opensAccessibilitySettings) {
            Intent(android.provider.Settings.ACTION_ACCESSIBILITY_SETTINGS)
        } else {
            Intent(this, MainActivity::class.java)
        }.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        val tap = PendingIntent.getActivity(this, 1996, target, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_menu_camera)
            .setContentTitle(notice.title)
            .setContentText(notice.text)
            .setContentIntent(tap)
            .setOngoing(true)
            .build()
    }

    companion object {
        private const val TAG = "ScreenshotService"
        private const val ACTION_STOP = "org.virtueinitiative.virtue.STOP"
        private const val CHANNEL_ID = "virtue_monitoring"
        private const val NOTIFICATION_ID = 1001

        @Volatile
        private var instance: ScreenshotService? = null

        /** Follows the Accessibility service connecting or disconnecting. A no-op when the service is not running. */
        fun refreshNotification() {
            runCatching { instance?.updateNotification() }
        }

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
