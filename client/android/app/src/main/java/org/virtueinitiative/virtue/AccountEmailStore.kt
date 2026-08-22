package org.virtueinitiative.virtue

import android.content.Context

/**
 * Remembers the email of the signed-in account so it can prefill the bug-report form's
 * contact-email field. Core's `AuthState`/`DeviceCredentials` don't carry the account
 * email, so this is app-layer state, not something read from the daemon — mirrors the
 * Windows client's `ClientState.email`.
 */
object AccountEmailStore {
    private const val PREFS = "virtue_account"
    private const val KEY_EMAIL = "email"

    fun save(context: Context, email: String) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit()
            .putString(KEY_EMAIL, email)
            .apply()
    }

    fun load(context: Context): String? {
        return context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getString(KEY_EMAIL, null)
    }

    fun clear(context: Context) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit()
            .clear()
            .apply()
    }
}
