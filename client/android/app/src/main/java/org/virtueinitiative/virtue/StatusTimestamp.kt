package org.virtueinitiative.virtue

import java.text.DateFormat
import java.text.SimpleDateFormat
import java.util.Calendar
import java.util.Date
import java.util.Locale

/**
 * How Status Details writes a timestamp (CLIENT-007): local time, then a
 * relative age in parentheses. The age wording is the same on every platform.
 */
object StatusTimestamp {
    const val NONE = "<none>"

    /**
     * `Ns ago`, `Nm ago`, `Nh Mm ago` or `Nd Nh ago`, rounded down, without a
     * second unit of zero, or `in the future`.
     */
    fun age(ms: Long, nowMs: Long): String {
        val deltaMs = nowMs - ms
        if (deltaMs < 0) return "in the future"
        val secs = deltaMs / 1000
        if (secs < 60) return "${secs}s ago"
        if (secs < 3600) return "${secs / 60}m ago"
        val (major, minor) =
            if (secs < 86_400) "${secs / 3600}h" to "${secs % 3600 / 60}m"
            else "${secs / 86_400}d" to "${secs % 86_400 / 3600}h"
        return if (minor.startsWith("0")) "$major ago" else "$major $minor ago"
    }

    /** `2026-11-11 19:32:05 (5m ago)`, or `<none>` for an absent value (zero or less). */
    fun full(ms: Long, nowMs: Long = System.currentTimeMillis()): String {
        if (ms <= 0L) return NONE
        val stamp = SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.US).format(Date(ms))
        return "$stamp (${age(ms, nowMs)})"
    }

    /** For a summary row: `7:32 PM (5m ago)`, with the date in front when it isn't today. */
    fun short(ms: Long, nowMs: Long = System.currentTimeMillis()): String {
        val time = DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(ms))
        val stamp =
            if (sameDay(ms, nowMs)) time
            else "${DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date(ms))}, $time"
        return "$stamp (${age(ms, nowMs)})"
    }

    private fun sameDay(a: Long, b: Long): Boolean {
        val first = Calendar.getInstance().apply { timeInMillis = a }
        val second = Calendar.getInstance().apply { timeInMillis = b }
        return first.get(Calendar.YEAR) == second.get(Calendar.YEAR) &&
            first.get(Calendar.DAY_OF_YEAR) == second.get(Calendar.DAY_OF_YEAR)
    }
}
