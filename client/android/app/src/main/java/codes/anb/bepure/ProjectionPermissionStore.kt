package codes.anb.virtue

import android.content.Context
import android.content.Intent

object ProjectionPermissionStore {
    private const val PREFS = "virtue_projection"

    @Volatile
    private var inMemoryGrant: Pair<Int, Intent>? = null

    fun save(context: Context, resultCode: Int, data: Intent): Boolean {
        inMemoryGrant = resultCode to Intent(data)
        // On Android 14+, each MediaProjection token is single-use. Persisting across process
        // restarts tends to produce stale grants, so keep only in-memory grant state.
        return true
    }

    fun load(@Suppress("UNUSED_PARAMETER") context: Context): Pair<Int, Intent>? {
        return inMemoryGrant?.let { (code, intent) -> code to Intent(intent) }
    }

    fun clear(context: Context) {
        inMemoryGrant = null
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit()
            .clear()
            .apply()
    }
}
