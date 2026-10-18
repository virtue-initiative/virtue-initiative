package org.virtueinitiative.virtue

import android.content.Context

/**
 * Google Play builds get their updates from Play, which forbids an app from
 * updating itself any other way. This stands in for the `direct` flavor's
 * self-updater so shared code can call it unconditionally; it never finds an
 * update.
 */
object AppUpdater {
    const val ACTION_INSTALL_UPDATE = "org.virtueinitiative.virtue.action.INSTALL_UPDATE"

    @Volatile
    var uiVisible = false

    fun schedule(@Suppress("UNUSED_PARAMETER") context: Context) {}

    fun checkSoon(@Suppress("UNUSED_PARAMETER") context: Context) {}

    fun readyVersion(@Suppress("UNUSED_PARAMETER") context: Context): String? = null

    fun installReadyUpdate(@Suppress("UNUSED_PARAMETER") context: Context): Boolean = false
}
