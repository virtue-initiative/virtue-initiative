package org.virtueinitiative.virtue

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val initError = NativeBridge.ensureInitialized(context)
        if (initError != null) return

        if (!NativeBridge.nativeIsLoggedIn()) return
        if (ProjectionPermissionStore.load(context) == null) return

        KeepAliveWorker.schedule(context)
        ScreenshotService.startFromStoredProjection(context, "boot")
    }
}
