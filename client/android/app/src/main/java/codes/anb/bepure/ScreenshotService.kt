package codes.anb.bepure

import android.app.*
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.PixelFormat
import android.hardware.display.DisplayManager
import android.hardware.display.VirtualDisplay
import android.media.Image
import android.media.ImageReader
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.os.Build
import android.os.IBinder
import android.util.DisplayMetrics
import android.view.WindowManager
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import kotlinx.coroutines.*
import java.io.ByteArrayOutputStream

class ScreenshotService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    private var captureJob: Job? = null
    private var mediaProjection: MediaProjection? = null
    private var virtualDisplay: VirtualDisplay? = null
    private var imageReader: ImageReader? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        startForeground(NOTIFICATION_ID, buildNotification("Monitoring active"))

        val initError = NativeBridge.ensureInitialized(this)
        if (initError != null) {
            updateNotification("Core init failed")
            stopSelf()
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                stopCaptureResources()
                stopSelf()
                return START_NOT_STICKY
            }
            ACTION_START -> {
                scope.launch {
                    startMonitoring(intent)
                }
            }
            else -> {
                scope.launch {
                    startMonitoring(intent)
                }
            }
        }

        return START_STICKY
    }

    override fun onDestroy() {
        captureJob?.cancel()
        stopCaptureResources()
        scope.cancel()
        super.onDestroy()
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        // Aggressive relaunch strategy: if system removes app task, ask AlarmManager to revive service.
        val restartIntent = Intent(applicationContext, ScreenshotService::class.java).apply {
            action = ACTION_START
            putExtra("source", "task_removed")
        }
        val pendingIntent = PendingIntent.getService(
            applicationContext,
            1995,
            restartIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        val alarmManager = getSystemService(Context.ALARM_SERVICE) as AlarmManager
        runCatching {
            alarmManager.setExactAndAllowWhileIdle(
                AlarmManager.RTC_WAKEUP,
                System.currentTimeMillis() + 5_000L,
                pendingIntent
            )
        }.onFailure {
            alarmManager.setAndAllowWhileIdle(
                AlarmManager.RTC_WAKEUP,
                System.currentTimeMillis() + 15_000L,
                pendingIntent
            )
        }
        super.onTaskRemoved(rootIntent)
    }

    private suspend fun startMonitoring(intent: Intent?) {
        val initError = NativeBridge.ensureInitialized(this)
        if (initError != null) {
            updateNotification("Core init failed")
            return
        }

        val loadedProjection = initProjectionFromIntentOrStore(intent)
        if (!loadedProjection) {
            updateNotification("Need screenshot permission from app")
            NativeBridge.nativeReportLog("missed_capture", "projection_permission_missing", null)
            return
        }

        if (captureJob?.isActive == true) {
            return
        }

        updateNotification("Monitoring active")
        captureJob = scope.launch {
            var lastSuccess = true
            while (isActive) {
                val delayMs = NativeBridge.nativeNextCaptureDelayMs(lastSuccess).coerceAtLeast(15_000)
                delay(delayMs)

                val pngBytes = runCatching { captureFrameAsPng() }.getOrNull()
                if (pngBytes == null) {
                    lastSuccess = false
                    NativeBridge.nativeReportLog("missed_capture", "capture_failed", null)
                    continue
                }

                val error = NativeBridge.nativeProcessCapture(pngBytes)
                lastSuccess = error == null
                if (error != null) {
                    NativeBridge.nativeReportLog("missed_capture", "upload_or_queue_failed", error)
                }
            }
        }
    }

    private fun initProjectionFromIntentOrStore(intent: Intent?): Boolean {
        val manager = getSystemService(Context.MEDIA_PROJECTION_SERVICE) as MediaProjectionManager

        val extrasCode = intent?.getIntExtra(EXTRA_RESULT_CODE, Int.MIN_VALUE) ?: Int.MIN_VALUE
        val extrasData = intent?.projectionDataExtra()

        val (resultCode, resultData) = if (extrasCode != Int.MIN_VALUE && extrasData != null) {
            ProjectionPermissionStore.save(this, extrasCode, extrasData)
            extrasCode to extrasData
        } else {
            ProjectionPermissionStore.load(this) ?: return false
        }

        return runCatching {
            mediaProjection = manager.getMediaProjection(resultCode, resultData)
            setupVirtualDisplay()
        }.isSuccess
    }

    private fun Intent.projectionDataExtra(): Intent? {
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            getParcelableExtra(EXTRA_RESULT_DATA, Intent::class.java)
        } else {
            @Suppress("DEPRECATION")
            getParcelableExtra(EXTRA_RESULT_DATA)
        }
    }

    private fun setupVirtualDisplay() {
        stopCaptureResources()

        val windowManager = getSystemService(Context.WINDOW_SERVICE) as WindowManager
        val metrics = DisplayMetrics()
        @Suppress("DEPRECATION")
        windowManager.defaultDisplay.getRealMetrics(metrics)

        imageReader = ImageReader.newInstance(
            metrics.widthPixels,
            metrics.heightPixels,
            PixelFormat.RGBA_8888,
            2
        )

        virtualDisplay = mediaProjection?.createVirtualDisplay(
            "bepure-capture",
            metrics.widthPixels,
            metrics.heightPixels,
            metrics.densityDpi,
            DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR,
            imageReader?.surface,
            null,
            null
        )
    }

    private fun captureFrameAsPng(): ByteArray? {
        repeat(12) {
            val image = imageReader?.acquireLatestImage()
            if (image != null) {
                return image.use { imageToPng(image) }
            }
            Thread.sleep(80)
        }
        return null
    }

    private fun imageToPng(image: Image): ByteArray {
        val width = image.width
        val height = image.height
        val plane = image.planes[0]

        val buffer = plane.buffer
        val pixelStride = plane.pixelStride
        val rowStride = plane.rowStride
        val rowPadding = rowStride - pixelStride * width

        val bitmap = Bitmap.createBitmap(
            width + rowPadding / pixelStride,
            height,
            Bitmap.Config.ARGB_8888
        )
        bitmap.copyPixelsFromBuffer(buffer)

        val cropped = Bitmap.createBitmap(bitmap, 0, 0, width, height)
        bitmap.recycle()

        val output = ByteArrayOutputStream()
        cropped.compress(Bitmap.CompressFormat.PNG, 100, output)
        cropped.recycle()

        return output.toByteArray()
    }

    private fun stopCaptureResources() {
        runCatching { virtualDisplay?.release() }
        runCatching { imageReader?.close() }
        runCatching { mediaProjection?.stop() }

        virtualDisplay = null
        imageReader = null
        mediaProjection = null
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val channel = NotificationChannel(
            CHANNEL_ID,
            "BePure monitoring",
            NotificationManager.IMPORTANCE_LOW
        )
        channel.description = "Background screenshot accountability monitoring"
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(channel)
    }

    private fun updateNotification(text: String) {
        val manager = getSystemService(NotificationManager::class.java)
        manager.notify(NOTIFICATION_ID, buildNotification(text))
    }

    private fun buildNotification(text: String): Notification {
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_menu_camera)
            .setContentTitle("BePure")
            .setContentText(text)
            .setOngoing(true)
            .build()
    }

    companion object {
        private const val ACTION_START = "codes.anb.bepure.START"
        private const val ACTION_STOP = "codes.anb.bepure.STOP"
        private const val EXTRA_RESULT_CODE = "projection_result_code"
        private const val EXTRA_RESULT_DATA = "projection_result_data"
        private const val CHANNEL_ID = "bepure_monitoring"
        private const val NOTIFICATION_ID = 1001

        fun start(context: Context, resultCode: Int, data: Intent) {
            val intent = Intent(context, ScreenshotService::class.java).apply {
                action = ACTION_START
                putExtra(EXTRA_RESULT_CODE, resultCode)
                putExtra(EXTRA_RESULT_DATA, data)
            }
            ContextCompat.startForegroundService(context, intent)
        }

        fun startFromStoredProjection(context: Context, source: String) {
            val intent = Intent(context, ScreenshotService::class.java).apply {
                action = ACTION_START
                putExtra("source", source)
            }
            ContextCompat.startForegroundService(context, intent)
        }

        fun stop(context: Context) {
            val intent = Intent(context, ScreenshotService::class.java).apply {
                action = ACTION_STOP
            }
            context.startService(intent)
        }
    }
}
