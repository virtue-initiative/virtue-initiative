package codes.anb.virtue

import android.app.Application

class VirtueApp : Application() {
    override fun onCreate() {
        super.onCreate()
        NativeBridge.ensureInitialized(this)
    }
}
