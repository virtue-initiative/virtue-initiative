package codes.anb.virtue

import android.content.Context

object NativeBridge {
    init {
        System.loadLibrary("virtue_android_rust")
    }

    @Volatile
    private var initialized = false

    private val initLock = Any()

    fun ensureInitialized(context: Context): String? {
        if (initialized) return null

        synchronized(initLock) {
            if (initialized) return null

            val error = nativeInit(
                context.filesDir.resolve("core-config").absolutePath,
                context.filesDir.resolve("core-data").absolutePath
            )
            if (error == null) {
                initialized = true
            }
            return error
        }
    }

    external fun nativeInit(configDir: String, dataDir: String): String?
    external fun nativeLogin(email: String, password: String, deviceName: String, intervalSeconds: Int): String?
    external fun nativeLogout(): String?
    external fun nativeIsLoggedIn(): Boolean
    external fun nativeGetDeviceId(): String?
    external fun nativeNextCaptureDelayMs(lastSuccess: Boolean): Long
    external fun nativeProcessCapture(pngBytes: ByteArray): String?
    external fun nativeReportLog(eventType: String, reason: String, detail: String?): String?
}
