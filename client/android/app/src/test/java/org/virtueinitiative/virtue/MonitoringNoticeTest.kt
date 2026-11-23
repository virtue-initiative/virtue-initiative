package org.virtueinitiative.virtue

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class MonitoringNoticeTest {
    private val pkg = "org.virtueinitiative.virtue"
    private val cls = "org.virtueinitiative.virtue.VirtueAccessibilityService"

    // CLIENT-030: the notification says monitoring stopped, and how to fix it, while Accessibility is off.
    @Test
    fun noticeFollowsAccessibility() {
        assertEquals("Monitoring active", MonitoringNotice.text(true).text)
        val off = MonitoringNotice.text(false)
        assertEquals("Monitoring stopped", off.title)
        assertEquals("Accessibility is off. Tap to turn it on.", off.text)
        assertTrue(off.opensAccessibilitySettings)
    }

    // CLIENT-030: a notification posted while Android would not show it comes back once it
    // would, and one the user dismissed while it could be seen stays dismissed.
    @Test
    fun noticeIsPostedAgainOnlyWhenAndroidDroppedIt() {
        // postedWhileHidden, shownNow, showing -> repost
        val cases = listOf(
            Triple(true, true, false) to true, // allowed after sign-in
            Triple(true, true, true) to false, // already back
            Triple(true, false, false) to false, // still off, or channel blocked
            Triple(false, true, false) to false, // the user dismissed it
            Triple(false, true, true) to false,
        )
        for ((input, expected) in cases) {
            val (postedWhileHidden, shownNow, showing) = input
            assertEquals(input.toString(), expected, MonitoringNotice.needsRepost(postedWhileHidden, shownNow, showing))
        }
    }

    // CLIENT-030: only a setting that no longer lists the service counts as turned off.
    @Test
    fun settingListMatchesShortAndFullNames() {
        assertTrue(MonitoringNotice.isListed("a.b/.C:org.virtueinitiative.virtue/.VirtueAccessibilityService", pkg, cls))
        assertTrue(MonitoringNotice.isListed("org.virtueinitiative.virtue/org.virtueinitiative.virtue.VirtueAccessibilityService", pkg, cls))
        assertFalse(MonitoringNotice.isListed("a.b/.C", pkg, cls))
        assertFalse(MonitoringNotice.isListed(null, pkg, cls))
        assertFalse(MonitoringNotice.isListed("", pkg, cls))
    }

    // CLIENT-008: a request that never answers ends at the deadline.
    @Test
    fun deadlineEndsAStuckRequest() = runBlocking {
        val start = System.nanoTime()
        assertNull(withDeadline(100) { Thread.sleep(3000); "late" })
        val elapsedMs = (System.nanoTime() - start) / 1_000_000
        assertTrue("returned after ${elapsedMs}ms, not at the deadline", elapsedMs < 1500)
        assertEquals("ok", withDeadline(2000) { "ok" })
    }
}
