package org.virtueinitiative.virtue

import android.app.Application

class VirtueApp : Application() {
    override fun onCreate() {
        super.onCreate()
        NativeBridge.ensureInitialized(this)
        AccessibilitySetupGuide.recordInstallSource(this)
        AppUpdater.schedule(this)
    }
}
