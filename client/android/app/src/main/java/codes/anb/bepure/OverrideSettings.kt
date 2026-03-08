package codes.anb.virtue

import android.content.Context

data class OverrideValues(
    val baseApiUrl: String?,
    val captureIntervalSeconds: String?,
    val batchWindowSeconds: String?
)

object OverrideSettings {
    private const val PREFS_NAME = "virtue_runtime_overrides"

    const val BASE_API_URL_KEY = "VIRTUE_BASE_API_URL"
    const val CAPTURE_INTERVAL_SECONDS_KEY = "VIRTUE_CAPTURE_INTERVAL_SECONDS"
    const val BATCH_WINDOW_SECONDS_KEY = "VIRTUE_BATCH_WINDOW_SECONDS"

    fun load(context: Context): OverrideValues {
        val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        return OverrideValues(
            baseApiUrl = prefs.getString(BASE_API_URL_KEY, null).normalized(),
            captureIntervalSeconds = prefs.getString(CAPTURE_INTERVAL_SECONDS_KEY, null).normalized(),
            batchWindowSeconds = prefs.getString(BATCH_WINDOW_SECONDS_KEY, null).normalized()
        )
    }

    fun save(context: Context, values: OverrideValues) {
        val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        prefs.edit()
            .putString(BASE_API_URL_KEY, values.baseApiUrl.normalized())
            .putString(CAPTURE_INTERVAL_SECONDS_KEY, values.captureIntervalSeconds.normalized())
            .putString(BATCH_WINDOW_SECONDS_KEY, values.batchWindowSeconds.normalized())
            .apply()
    }

    private fun String?.normalized(): String? {
        val trimmed = this?.trim().orEmpty()
        return if (trimmed.isEmpty()) null else trimmed
    }
}
