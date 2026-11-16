package org.virtueinitiative.virtue

import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

// CLIENT-007: timestamps show local time and a relative age that reads Ns, Nm,
// Nh Mm or Nd Nh ago, and absent values show <none>.
class StatusTimestampTest {
    private val second = 1000L
    private val minute = 60 * second
    private val hour = 60 * minute
    private val day = 24 * hour
    private val now = 1_800_000_000_000L

    @Test
    fun formatsRelativeAges() {
        val cases = listOf(
            -1L to "in the future",
            0L to "0s ago",
            59 * second + 999 to "59s ago",
            minute to "1m ago",
            59 * minute + 59 * second to "59m ago",
            hour to "1h ago",
            4 * hour + 54 * minute to "4h 54m ago",
            23 * hour + 59 * minute + 59 * second to "23h 59m ago",
            day to "1d ago",
            2 * day + 3 * hour + 30 * minute to "2d 3h ago",
            40 * day + 10 * hour to "40d 10h ago",
        )
        for ((deltaMs, expected) in cases) {
            assertEquals("$deltaMs ms", expected, StatusTimestamp.age(now - deltaMs, now))
        }
    }

    @Test
    fun fullTimestampIsLocalTimeFollowedByTheAge() {
        val then = now - 5 * minute
        val local = SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.US).format(Date(then))
        assertEquals("$local (5m ago)", StatusTimestamp.full(then, now))
    }

    @Test
    fun shortTimestampEndsWithTheAge() {
        val then = now - 4 * hour - 54 * minute
        assertTrue(StatusTimestamp.short(then, now).endsWith(" (4h 54m ago)"))
    }

    @Test
    fun absentTimestampShowsNone() {
        assertEquals("<none>", StatusTimestamp.full(0, now))
    }
}
