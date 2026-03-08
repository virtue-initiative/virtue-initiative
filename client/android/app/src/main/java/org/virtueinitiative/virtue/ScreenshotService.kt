package org.virtueinitiative.virtue

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
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.util.DisplayMetrics
import android.util.Log
import android.view.WindowManager
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import kotlinx.coroutines.*
import java.io.ByteArrayOutputStream

class ScreenshotService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val startLock = Any()

    private var captureJob: Job? = null
    @Volatile
    private var startInProgress = false
    private val projectionCallbackHandler by lazy { Handler(Looper.getMainLooper()) }
    private var mediaProjection: MediaProjection? = null
    private var projectionCallback: MediaProjection.Callback? = null
    private var virtualDisplay: VirtualDisplay? = null
    private var imageReader: ImageReader? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        startForeground(NOTIFICATION_ID, buildNotification("Monitoring active"))

        val overrides = OverrideSettings.load(this)
        Log.i(
            TAG,
            "Runtime overrides: baseApiUrl=${overrides.baseApiUrl ?: "<default>"}, " +
                "captureIntervalSeconds=${overrides.captureIntervalSeconds ?: "<default>"}, " +
                "batchWindowSeconds=${overrides.batchWindowSeconds ?: "<default>"}"
        )

        val initError = NativeBridge.ensureInitialized(this)
        if (initError != null) {
            updateNotification("Core init failed")
            stopSelf()
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                synchronized(startLock) {
                    startInProgress = false
                }
                stopCaptureResources()
                stopSelf()
                return START_NOT_STICKY
            }
            ACTION_START -> {
                launchMonitoringIfNeeded(intent)
            }
            else -> {
                launchMonitoringIfNeeded(intent)
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
        val triggerAtMillis = System.currentTimeMillis() + 5_000L

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && !alarmManager.canScheduleExactAlarms()) {
            alarmManager.setAndAllowWhileIdle(
                AlarmManager.RTC_WAKEUP,
                triggerAtMillis + 10_000L,
                pendingIntent
            )
        } else {
            runCatching {
                alarmManager.setExactAndAllowWhileIdle(
                    AlarmManager.RTC_WAKEUP,
                    triggerAtMillis,
                    pendingIntent
                )
            }.onFailure {
                alarmManager.setAndAllowWhileIdle(
                    AlarmManager.RTC_WAKEUP,
                    triggerAtMillis + 10_000L,
                    pendingIntent
                )
            }
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

        val initialPngBytes = runCatching { captureFrameAsPng() }.getOrNull()
        if (initialPngBytes == null) {
            Log.w(TAG, "Initial capture frame unavailable; will retry on schedule")
        }

        updateNotification("Monitoring active")
        captureJob = scope.launch {
            var lastSuccess = true
            var lastPngBytes: ByteArray? = initialPngBytes
            while (isActive) {
                val delayMs = NativeBridge.nativeNextCaptureDelayMs(lastSuccess).coerceAtLeast(15_000)
                Log.d(TAG, "Next capture in ${delayMs}ms (lastSuccess=$lastSuccess)")
                delay(delayMs)

                val freshPngBytes = runCatching { captureFrameAsPng() }.getOrNull()
                if (freshPngBytes != null) {
                    lastPngBytes = freshPngBytes
                } else if (lastPngBytes != null) {
                    Log.w(TAG, "No fresh frame available; reusing last successful frame")
                }

                val pngBytes = freshPngBytes ?: lastPngBytes
                if (pngBytes == null) {
                    lastSuccess = false
                    Log.w(TAG, "Capture failed: no frame available")
                    NativeBridge.nativeReportLog("missed_capture", "capture_failed", null)
                    continue
                }

                val error = NativeBridge.nativeProcessCapture(pngBytes)
                lastSuccess = error == null
                if (error != null) {
                    Log.w(TAG, "Capture processed with error: $error")
                    NativeBridge.nativeReportLog("missed_capture", "upload_or_queue_failed", error)
                }
            }
        }
    }

    private fun launchMonitoringIfNeeded(intent: Intent?) {
        synchronized(startLock) {
            if (captureJob?.isActive == true || startInProgress) {
                return
            }
            startInProgress = true
        }

        scope.launch {
            try {
                startMonitoring(intent)
            } finally {
                synchronized(startLock) {
                    startInProgress = false
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

        val initialized = runCatching {
            stopCaptureResources()
            mediaProjection = manager.getMediaProjection(resultCode, resultData)
                ?: throw IllegalStateException("MediaProjection token was rejected")
            registerProjectionCallback(mediaProjection!!)
            setupVirtualDisplay()
        }.onFailure { err ->
            Log.e(TAG, "Failed to initialize MediaProjection", err)
        }.isSuccess

        if (!initialized) {
            ProjectionPermissionStore.clear(this)
            stopCaptureResources()
        }

        return initialized
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
        stopDisplayResources()

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
            "virtue-capture",
            metrics.widthPixels,
            metrics.heightPixels,
            metrics.densityDpi,
            DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR,
            imageReader?.surface,
            null,
            null
        ) ?: throw IllegalStateException("Failed to create VirtualDisplay")
    }

    private fun registerProjectionCallback(projection: MediaProjection) {
        unregisterProjectionCallback()

        val callback = object : MediaProjection.Callback() {
            override fun onStop() {
                super.onStop()
                Log.i(TAG, "MediaProjection stopped by system")
                scope.launch {
                    captureJob?.cancel()
                    captureJob = null
                    stopDisplayResources()
                    mediaProjection = null
                    projectionCallback = null
                    ProjectionPermissionStore.clear(this@ScreenshotService)
                    updateNotification("Capture permission ended")
                    NativeBridge.nativeReportLog("missed_capture", "projection_stopped", null)
                }
            }
        }

        projection.registerCallback(callback, projectionCallbackHandler)
        projectionCallback = callback
    }

    private fun unregisterProjectionCallback() {
        val projection = mediaProjection
        val callback = projectionCallback
        if (projection != null && callback != null) {
            runCatching { projection.unregisterCallback(callback) }
        }
        projectionCallback = null
    }

    private fun captureFrameAsPng(): ByteArray? {
        repeat(20) {
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

    private fun stopDisplayResources() {
        runCatching { virtualDisplay?.release() }
        runCatching { imageReader?.close() }

        virtualDisplay = null
        imageReader = null
    }

    private fun stopCaptureResources() {
        stopDisplayResources()
        unregisterProjectionCallback()
        runCatching { mediaProjection?.stop() }
        mediaProjection = null
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val channel = NotificationChannel(
            CHANNEL_ID,
            "Virtue monitoring",
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
            .setContentTitle("Virtue")
            .setContentText(text)
            .setOngoing(true)
            .build()
    }

    companion object {
        private const val TAG = "ScreenshotService"
        private const val ACTION_START = "org.virtueinitiative.virtue.START"
        private const val ACTION_STOP = "org.virtueinitiative.virtue.STOP"
        private const val EXTRA_RESULT_CODE = "projection_result_code"
        private const val EXTRA_RESULT_DATA = "projection_result_data"
        private const val CHANNEL_ID = "virtue_monitoring"
        private const val NOTIFICATION_ID = 1001

        fun start(context: Context, resultCode: Int, data: Intent): String? {
            val intent = Intent(context, ScreenshotService::class.java).apply {
                action = ACTION_START
                putExtra(EXTRA_RESULT_CODE, resultCode)
                putExtra(EXTRA_RESULT_DATA, data)
            }
            return runCatching {
                ContextCompat.startForegroundService(context, intent)
            }.exceptionOrNull()?.message
        }

        fun startFromStoredProjection(context: Context, source: String): String? {
            val intent = Intent(context, ScreenshotService::class.java).apply {
                action = ACTION_START
                putExtra("source", source)
            }
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
    }
}
