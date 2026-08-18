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

            // The application context is passed through to Rust so
            // rustls-platform-verifier can reach Android's trust store; use the
            // application context rather than an Activity so the verifier never
            // holds a reference to a destroyed Activity.
            val appContext = context.applicationContext

            var error = nativeInit(
                appContext,
                context.filesDir.resolve("core-config").absolutePath,
                dataDir.absolutePath
            )
            if (error != null && error.contains("serialization error")) {
                // Corrupted state files — wipe and retry once
                android.util.Log.w("NativeBridge", "Init serialization error, wiping core-data: $error")
                dataDir.deleteRecursively()
                error = nativeInit(
                    appContext,
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
        context: Context,
        configDir: String,
        dataDir: String
    ): String?
    external fun nativeLogin(email: String, password: String, deviceName: String): String?
    external fun nativeLogout(): String?
    external fun nativeIsLoggedIn(): Boolean
    external fun nativeGetDeviceId(): String?
    external fun nativeRunDaemonLoop(): String?
    external fun nativeStopDaemon(): String?
    external fun nativeNoteUserStop(source: String): String?
    external fun nativeGetStatusJson(): String
}
