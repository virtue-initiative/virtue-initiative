package org.virtueinitiative.virtue

import android.app.AlarmManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
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
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import java.io.ByteArrayOutputStream

class ScreenshotService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val startLock = Any()

    @Volatile
    private var startInProgress = false
    private var daemonJob: Job? = null

    private val projectionCallbackHandler by lazy { Handler(Looper.getMainLooper()) }
    private var mediaProjection: MediaProjection? = null
    private var projectionCallback: MediaProjection.Callback? = null
    private var virtualDisplay: VirtualDisplay? = null
    private var imageReader: ImageReader? = null
    private var lastCapturedFrame: ByteArray? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        activeService = this

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
                stopDaemonLoop()
                stopCaptureResources()
                stopSelf()
                return START_NOT_STICKY
            }
            ACTION_START -> launchMonitoringIfNeeded(intent)
            else -> launchMonitoringIfNeeded(intent)
        }

        return START_STICKY
    }

    override fun onDestroy() {
        stopDaemonLoop()
        stopCaptureResources()
        scope.cancel()
        activeService = null
        super.onDestroy()
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
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

    private fun launchMonitoringIfNeeded(intent: Intent?) {
        synchronized(startLock) {
            if (startInProgress || daemonJob?.isActive == true) {
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

    private fun startMonitoring(intent: Intent?) {
        val initError = NativeBridge.ensureInitialized(this)
        if (initError != null) {
            updateNotification("Core init failed")
            return
        }

        val hasProjection = initProjectionFromIntentOrStore(intent)
        startDaemonLoop()

        if (hasProjection) {
            updateNotification("Monitoring active")
        } else {
            updateNotification("Monitoring active (grant screenshot permission in app)")
        }
    }

    private fun startDaemonLoop() {
        if (daemonJob?.isActive == true) {
            return
        }

        daemonJob = scope.launch(Dispatchers.IO) {
            val error = NativeBridge.nativeRunDaemonLoop()
            if (error != null) {
                Log.e(TAG, "Native daemon exited with error: $error")
                updateNotification("Monitoring paused: $error")
            }
        }
    }

    private fun stopDaemonLoop() {
        runCatching { NativeBridge.nativeStopDaemon() }
            .exceptionOrNull()
            ?.let { err -> Log.w(TAG, "Failed to request daemon stop", err) }
        daemonJob?.cancel()
        daemonJob = null
    }

    private fun initProjectionFromIntentOrStore(intent: Intent?): Boolean {
        val manager = getSystemService(Context.MEDIA_PROJECTION_SERVICE) as MediaProjectionManager

        val extrasCode = intent?.getIntExtra(EXTRA_RESULT_CODE, Int.MIN_VALUE) ?: Int.MIN_VALUE
        val extrasData = intent?.projectionDataExtra()

        val projectionPair = if (extrasCode != Int.MIN_VALUE && extrasData != null) {
            ProjectionPermissionStore.save(this, extrasCode, extrasData)
            extrasCode to extrasData
        } else {
            ProjectionPermissionStore.load(this)
        } ?: return false

        val initialized = runCatching {
            val (resultCode, resultData) = projectionPair
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
                stopDisplayResources()
                mediaProjection = null
                projectionCallback = null
                ProjectionPermissionStore.clear(this@ScreenshotService)
                updateNotification("Capture permission ended")
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

    private fun captureStatusForDaemonInternal(): Int {
        if (mediaProjection == null || virtualDisplay == null || imageReader == null) {
            return CAPTURE_STATUS_PERMISSION_MISSING
        }
        return CAPTURE_STATUS_READY
    }

    private fun capturePngForDaemonInternal(): ByteArray? {
        if (captureStatusForDaemonInternal() != CAPTURE_STATUS_READY) {
            return null
        }

        val fresh = runCatching { captureFrameAsPng() }.getOrNull()
        if (fresh != null) {
            lastCapturedFrame = fresh
            return fresh
        }

        return lastCapturedFrame
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
        lastCapturedFrame = null
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

        private const val CAPTURE_STATUS_READY = 0
        private const val CAPTURE_STATUS_PERMISSION_MISSING = 1
        private const val CAPTURE_STATUS_SESSION_UNAVAILABLE = 2

        @Volatile
        private var activeService: ScreenshotService? = null

        @JvmStatic
        fun captureStatusForDaemon(): Int {
            val service = activeService ?: return CAPTURE_STATUS_SESSION_UNAVAILABLE
            return service.captureStatusForDaemonInternal()
        }

        @JvmStatic
        fun capturePngForDaemon(): ByteArray? {
            return activeService?.capturePngForDaemonInternal()
        }

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
