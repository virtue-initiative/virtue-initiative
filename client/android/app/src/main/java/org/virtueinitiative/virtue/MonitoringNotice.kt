package org.virtueinitiative.virtue

import android.content.Context
import android.provider.Settings
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull

/** What the persistent notification says (CLIENT-030). */
data class NoticeText(val title: String, val text: String, val opensAccessibilitySettings: Boolean)

object MonitoringNotice {
    fun text(accessibilityConnected: Boolean): NoticeText =
        if (accessibilityConnected) {
            NoticeText("Virtue", "Monitoring active", false)
        } else {
            NoticeText("Monitoring stopped", "Accessibility is off. Tap to turn it on.", true)
        }

    /**
     * True when [enabledServices] (the colon-separated value of
     * `Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES`) lists [service]. Entries come in
     * both the short (`pkg/.Cls`) and full (`pkg/pkg.Cls`) forms.
     */
    fun isListed(enabledServices: String?, pkg: String, cls: String): Boolean =
        enabledServices.orEmpty().split(':').any { entry ->
            val parts = entry.trim().split('/')
            if (parts.size != 2) return@any false
            val name = if (parts[1].startsWith(".")) parts[0] + parts[1] else parts[1]
            parts[0] == pkg && name == cls
        }

    /** Whether the user really has Virtue Screen Monitor switched on in Android's settings. */
    fun accessibilitySettingOn(context: Context): Boolean {
        val enabled = Settings.Secure.getInt(context.contentResolver, Settings.Secure.ACCESSIBILITY_ENABLED, 0) == 1
        if (!enabled) return false
        val list = Settings.Secure.getString(context.contentResolver, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES)
        return isListed(list, context.packageName, VirtueAccessibilityService::class.java.name)
    }
}

/** Test screenshot deadline (CLIENT-008). Returns null when [block] has not finished by [timeoutMs]. */
suspend fun <T> withDeadline(timeoutMs: Long, block: () -> T): T? = withContext(Dispatchers.IO) {
    // The blocking call itself cannot be cancelled, but waiting for it can.
    val work = async { block() }
    withTimeoutOrNull(timeoutMs) { work.await() }
}
