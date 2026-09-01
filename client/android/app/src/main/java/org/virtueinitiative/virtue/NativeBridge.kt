package org.virtueinitiative.virtue

import android.content.Context

object NativeBridge {
    init {
        System.loadLibrary("virtue_android")
    }

    @Volatile
    private var initialized = false

    private val initLock = Any()

    fun ensureInitialized(context: Context): String? {
        if (initialized) return null

        synchronized(initLock) {
            if (initialized) return null
            val dataDir = context.filesDir.resolve("core-data")

            var error = nativeInit(
                context.filesDir.resolve("core-config").absolutePath,
                dataDir.absolutePath
            )
            if (error != null && error.contains("serialization error")) {
                // Corrupted state files — wipe and retry once
                android.util.Log.w("NativeBridge", "Init serialization error, wiping core-data: $error")
                dataDir.deleteRecursively()
                error = nativeInit(
                    context.filesDir.resolve("core-config").absolutePath,
                    dataDir.absolutePath
                )
            }
            if (error == null) {
                initialized = true
            }
            return error
        }
    }

    external fun nativeInit(
        configDir: String,
        dataDir: String
    ): String?
    external fun nativeLogin(email: String, password: String, deviceName: String): String?
    /** CORE-020. JSON: `{"userCode": …, "expiresAtMs": …, "intervalSeconds": …}`, or `{"error": …}`. */
    external fun nativeBeginCodeLogin(deviceName: String): String

    /** CORE-021. JSON: `{"status": "pending"|"approved"|"expired", "deviceId": …}`, or `{"error": …}`. */
    external fun nativePollCodeLogin(): String

    external fun nativeLogout(): String?
    external fun nativeIsLoggedIn(): Boolean
    external fun nativeGetDeviceId(): String?
    external fun nativeRunDaemonLoop(): String?
    external fun nativeStopDaemon(): String?
    /** JSON: `{"outcome": …, "message": …}`, or `{"error": …}` on failure. */
    external fun nativeForceCapture(): String
    external fun nativeNoteUserStop(source: String): String?
    external fun nativeGetStatusJson(): String
    external fun nativeReportIssue(
        message: String,
        contactEmail: String,
        includeLogs: Boolean,
        platformDetails: String
    ): String?
}
