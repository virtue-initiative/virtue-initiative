package org.virtueinitiative.virtue

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val initError = NativeBridge.ensureInitialized(context)
        if (initError != null) return

        if (!NativeBridge.nativeIsLoggedIn()) return

        // The AccessibilityService is restarted automatically by Android after boot.
        // Just ensure the foreground notification service is alive to keep the process warm.
        KeepAliveWorker.schedule(context)
        ScreenshotService.start(context)
    }
}
