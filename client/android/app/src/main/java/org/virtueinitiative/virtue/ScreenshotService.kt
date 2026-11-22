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
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel

// Lightweight foreground service that keeps the process alive and holds the notification.
// Screen capture is performed by VirtueAccessibilityService.takeScreenshot().
class ScreenshotService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    // False once the service is stopping, so nothing posts a notification
    // Android has already cancelled with the service (CLIENT-030).
    private var foreground = false
    // The last post went out while Android would not show it, so it was dropped.
    private var postedWhileHidden = false

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
            foreground = true
            postedWhileHidden = !notificationsShown()
        } catch (e: IllegalStateException) {
            Log.w(TAG, "Foreground service start refused", e)
            stop()
            return
        }

        val initError = NativeBridge.ensureInitialized(this)
        if (initError != null) {
            Log.e(TAG, "Core init failed: $initError")
            stop()
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            stop()
            return START_NOT_STICKY
        }
        // Every start lands here: the app coming to the front, KeepAliveWorker,
        // and the first start right after onCreate (which finds nothing to do).
        repostIfMissing()
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

    private fun stop() {
        foreground = false
        stopSelf()
    }

    /** Re-posts the notification for the current Accessibility state (CLIENT-030). */
    fun updateNotification() {
        if (!foreground) return
        getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, buildNotification())
        postedWhileHidden = !notificationsShown()
    }

    /** Posts the notification again if Android dropped it while notifications were off (CLIENT-030). */
    private fun repostIfMissing() {
        if (!foreground) return
        val shownNow = notificationsShown()
        val showing = getSystemService(NotificationManager::class.java)
            .activeNotifications.any { it.id == NOTIFICATION_ID }
        if (MonitoringNotice.needsRepost(postedWhileHidden, shownNow, showing)) {
            updateNotification()
        } else if (shownNow) {
            postedWhileHidden = false
        }
    }

    /** Whether Android would show a notification on our channel right now. */
    private fun notificationsShown(): Boolean {
        val manager = getSystemService(NotificationManager::class.java)
        if (!NotificationManagerCompat.from(this).areNotificationsEnabled()) return false
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return true
        val channel = manager.getNotificationChannel(CHANNEL_ID) ?: return true
        return channel.importance != NotificationManager.IMPORTANCE_NONE
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
            // Android 12+ otherwise holds a new service's notification back for
            // up to 10 s, and CLIENT-030 wants it there as soon as the user looks.
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
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
