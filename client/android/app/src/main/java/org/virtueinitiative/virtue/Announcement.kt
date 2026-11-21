package org.virtueinitiative.virtue

import org.json.JSONObject

/**
 * A message for the person using this device, from `announcement` in the core's
 * status JSON (CORE-010, API-068). The main screen shows it below its buttons
 * while signed in (CLIENT-033).
 */
data class Announcement(val message: String, val url: String?, val level: Level) {
    enum class Level { INFO, WARNING, URGENT }

    val prominent: Boolean get() = level != Level.INFO

    companion object {
        /** The announcement in [status], or null when it has none. */
        fun fromStatus(status: JSONObject): Announcement? {
            val obj = status.optJSONObject("announcement") ?: return null
            val message = obj.optString("message", "").trim()
            if (message.isEmpty()) return null
            // `optString` renders a JSON null as "null", so check for it first.
            val url = if (obj.isNull("url")) null else obj.optString("url", "").trim().ifEmpty { null }
            // A level from a newer core reads as info.
            val level = when (obj.optString("level", "")) {
                "warning" -> Level.WARNING
                "urgent" -> Level.URGENT
                else -> Level.INFO
            }
            return Announcement(message, url, level)
        }
    }
}
