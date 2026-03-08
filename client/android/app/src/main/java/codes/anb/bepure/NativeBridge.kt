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
            val overrides = OverrideSettings.load(context)
            val baseApiUrl = overrides.baseApiUrl ?: ""
            val captureIntervalSeconds = overrides.captureIntervalSeconds ?: ""
            val batchWindowSeconds = overrides.batchWindowSeconds ?: ""

            val error = nativeInit(
                context.filesDir.resolve("core-config").absolutePath,
                context.filesDir.resolve("core-data").absolutePath,
                baseApiUrl,
                captureIntervalSeconds,
                batchWindowSeconds
            )
            if (error == null) {
                initialized = true
            }
            return error
        }
    }

    fun applyOverrides(context: Context): String? {
        val overrides = OverrideSettings.load(context)
        val baseApiUrl = overrides.baseApiUrl ?: ""
        val captureIntervalSeconds = overrides.captureIntervalSeconds ?: ""
        val batchWindowSeconds = overrides.batchWindowSeconds ?: ""

        synchronized(initLock) {
            if (!initialized) {
                val error = nativeInit(
                    context.filesDir.resolve("core-config").absolutePath,
                    context.filesDir.resolve("core-data").absolutePath,
                    baseApiUrl,
                    captureIntervalSeconds,
                    batchWindowSeconds
                )
                if (error == null) {
                    initialized = true
                }
                return error
            }

            return nativeSetOverrides(baseApiUrl, captureIntervalSeconds, batchWindowSeconds)
        }
    }

    external fun nativeInit(
        configDir: String,
        dataDir: String,
        baseApiUrl: String,
        captureIntervalSeconds: String,
        batchWindowSeconds: String
    ): String?
    external fun nativeSetOverrides(
        baseApiUrl: String,
        captureIntervalSeconds: String,
        batchWindowSeconds: String
    ): String?
    external fun nativeLogin(email: String, password: String, deviceName: String): String?
    external fun nativeLogout(): String?
    external fun nativeIsLoggedIn(): Boolean
    external fun nativeGetDeviceId(): String?
    external fun nativeNextCaptureDelayMs(lastSuccess: Boolean): Long
    external fun nativeProcessCapture(pngBytes: ByteArray): String?
    external fun nativeReportLog(eventType: String, reason: String, detail: String?): String?
}
