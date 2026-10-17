package org.virtueinitiative.virtue

import android.content.Context

/**
 * Whether the user agreed to the Accessibility disclosure that
 * [AccessibilitySetupGuide] shows before its setup steps.
 *
 * Virtue isn't an accessibility tool, so Google Play requires an in-app
 * disclosure of what the Accessibility API is used for, and an affirmative
 * choice, before the app sends the user to turn its service on. Raise
 * [VERSION] when the disclosure changes enough that users should see it again.
 */
object AccessibilityConsent {
    private const val PREFS = "accessibility_consent"
    private const val KEY_VERSION = "accepted_version"
    private const val VERSION = 1

    /** Set when the user taps "No Thanks"; cleared when they review it again. */
    @Volatile
    var declined = false

    fun isGiven(context: Context): Boolean = prefs(context).getInt(KEY_VERSION, 0) >= VERSION

    fun give(context: Context) {
        declined = false
        prefs(context).edit().putInt(KEY_VERSION, VERSION).apply()
    }

    private fun prefs(context: Context) = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
}
