package org.virtueinitiative.virtue

import org.json.JSONObject

/**
 * The stable-release feed served at [BuildConfig.UPDATE_MANIFEST_URL], built by
 * `landing/scripts/build-android-update.mjs` from the latest stable GitHub
 * release:
 *
 * ```json
 * {"version": "0.1.5", "url": "https://.../virtue-android-....apk", "sha256": "<64 hex>"}
 * ```
 *
 * An empty object (or anything missing a field) means "no update offered".
 */
data class UpdateManifest(val version: String, val url: String, val sha256: String) {
    companion object {
        private val SHA256_HEX = Regex("^[0-9a-f]{64}$")

        /** Parses [json], or returns null if it doesn't describe a usable update. */
        fun parse(json: String, allowInsecureUrl: Boolean = false): UpdateManifest? {
            val obj = runCatching { JSONObject(json) }.getOrNull() ?: return null
            val version = obj.optString("version").trim()
            val url = obj.optString("url").trim()
            val sha256 = obj.optString("sha256").trim().lowercase()
            if (parseVersion(version) == null) return null
            if (!url.startsWith("https://") && !(allowInsecureUrl && url.startsWith("http://"))) {
                return null
            }
            if (!SHA256_HEX.matches(sha256)) return null
            return UpdateManifest(version, url, sha256)
        }

        /** Dot-separated non-negative integers ("0.1.4"), or null for anything else. */
        fun parseVersion(version: String): List<Int>? {
            if (version.isEmpty()) return null
            val parts = version.split('.')
            if (parts.any { it.isEmpty() || !it.all(Char::isDigit) }) return null
            return parts.map { it.toIntOrNull() ?: return null }
        }

        /** Whether [candidate] is a strictly newer version than [current]. */
        fun isNewer(candidate: String, current: String): Boolean {
            val a = parseVersion(candidate) ?: return false
            val b = parseVersion(current) ?: return false
            for (i in 0 until maxOf(a.size, b.size)) {
                val x = a.getOrElse(i) { 0 }
                val y = b.getOrElse(i) { 0 }
                if (x != y) return x > y
            }
            return false
        }
    }
}
