package org.virtueinitiative.virtue

import android.accessibilityservice.AccessibilityService
import android.graphics.Bitmap
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.accessibility.AccessibilityEvent
import androidx.annotation.RequiresApi
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import java.io.ByteArrayOutputStream

class VirtueAccessibilityService : AccessibilityService() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private var daemonJob: Job? = null

    @Volatile
    var paused = false

    override fun onServiceConnected() {
        super.onServiceConnected()
        activeService = this
        Log.i(TAG, "Accessibility service connected")

        val initError = NativeBridge.ensureInitialized(this)
        if (initError != null) {
            Log.e(TAG, "Core init failed: $initError")
            return
        }

        if (NativeBridge.nativeIsLoggedIn() && !paused) {
            startDaemonLoop()
        }
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        // We only need the accessibility permission for takeScreenshot(); no event processing needed.
    }

    override fun onInterrupt() {
        Log.i(TAG, "Accessibility service interrupted")
    }

    override fun onDestroy() {
        stopDaemonLoop()
        scope.cancel()
        activeService = null
        super.onDestroy()
        Log.i(TAG, "Accessibility service destroyed")
    }

    fun startDaemonLoop() {
        if (daemonJob?.isActive == true) return
        paused = false

        daemonJob = scope.launch(Dispatchers.IO) {
            val error = NativeBridge.nativeRunDaemonLoop()
            if (error != null) {
                Log.e(TAG, "Native daemon exited with error: $error")
            }
        }
    }

    fun stopDaemonLoop() {
        runCatching { NativeBridge.nativeStopDaemon() }
            .exceptionOrNull()
            ?.let { err -> Log.w(TAG, "Failed to request daemon stop", err) }
        daemonJob?.cancel()
        daemonJob = null
    }

    fun pauseInternal() {
        paused = true
        stopDaemonLoop()
    }

    private fun captureStatusInternal(): Int {
        // The service being connected and enabled IS the permission
        return CAPTURE_STATUS_READY
    }

    @RequiresApi(Build.VERSION_CODES.R)
    private fun capturePngInternal(): ByteArray? {
        if (paused) return null

        var result: ByteArray? = null
        val latch = java.util.concurrent.CountDownLatch(1)

        // takeScreenshot must be called on the main thread handler
        Handler(Looper.getMainLooper()).post {
            takeScreenshot(
                android.view.Display.DEFAULT_DISPLAY,
                mainExecutor,
                object : TakeScreenshotCallback {
                    override fun onSuccess(screenshot: ScreenshotResult) {
                        try {
                            val hwBitmap = Bitmap.wrapHardwareBuffer(
                                screenshot.hardwareBuffer,
                                screenshot.colorSpace
                            )
                            val bitmap = hwBitmap?.copy(Bitmap.Config.ARGB_8888, false)
                            hwBitmap?.recycle()
                            screenshot.hardwareBuffer.close()

                            if (bitmap != null) {
                                val out = ByteArrayOutputStream()
                                bitmap.compress(Bitmap.CompressFormat.PNG, 100, out)
                                bitmap.recycle()
                                result = out.toByteArray()
                            }
                        } finally {
                            latch.countDown()
                        }
                    }

                    override fun onFailure(errorCode: Int) {
                        Log.w(TAG, "takeScreenshot failed with code $errorCode")
                        latch.countDown()
                    }
                }
            )
        }

        latch.await(5, java.util.concurrent.TimeUnit.SECONDS)
        return result
    }

    companion object {
        private const val TAG = "VirtueA11y"
        private const val CAPTURE_STATUS_READY = 0
        private const val CAPTURE_STATUS_PERMISSION_MISSING = 1

        @Volatile
        private var activeService: VirtueAccessibilityService? = null

        fun isEnabled(): Boolean = activeService?.let { !it.paused && it.daemonJob?.isActive == true } == true
        fun isPaused(): Boolean = activeService?.paused == true
        fun isConnected(): Boolean = activeService != null

        fun pause() {
            activeService?.pauseInternal()
        }

        fun resume() {
            activeService?.startDaemonLoop()
        }

        @JvmStatic
        fun captureStatusForDaemon(): Int {
            val svc = activeService ?: return CAPTURE_STATUS_PERMISSION_MISSING
            return svc.captureStatusInternal()
        }

        @JvmStatic
        fun capturePngForDaemon(): ByteArray? {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return null
            return activeService?.capturePngInternal()
        }
    }
}
