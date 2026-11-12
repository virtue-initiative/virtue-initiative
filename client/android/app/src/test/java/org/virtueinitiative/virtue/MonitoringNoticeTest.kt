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
        assertNull(withDeadline(100) { Thread.sleep(2000); "late" })
        assertEquals("ok", withDeadline(2000) { "ok" })
    }
}
