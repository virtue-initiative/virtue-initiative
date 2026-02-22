package codes.anb.bepure

import android.app.Application

class BePureApp : Application() {
    override fun onCreate() {
        super.onCreate()
        NativeBridge.ensureInitialized(this)
    }
}
