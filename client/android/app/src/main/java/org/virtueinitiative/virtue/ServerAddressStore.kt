package org.virtueinitiative.virtue

import android.content.Context

/**
 * Remembers the self-hosted server address last signed in to, so the next sign-in
 * offers it again (CLIENT-034). Kept apart from [AccountEmailStore] because signing
 * out forgets the email but not the server.
 */
object ServerAddressStore {
    private const val PREFS = "virtue_server"
    private const val KEY_ADDRESS = "address"

    /** Saves [address], or forgets it when null (the default server). */
    fun save(context: Context, address: String?) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit()
            .apply { if (address == null) remove(KEY_ADDRESS) else putString(KEY_ADDRESS, address) }
            .apply()
    }

    fun load(context: Context): String? {
        return context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getString(KEY_ADDRESS, null)
    }
}
