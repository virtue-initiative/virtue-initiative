package org.virtueinitiative.virtue

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

// CORE-010, CLIENT-033: the announcement is read from status, is absent when
// the core sends none, and an unknown level reads as info.
class AnnouncementTest {
    private fun parse(extra: String) =
        Announcement.fromStatus(JSONObject("""{"is_authenticated":true$extra}"""))

    @Test
    fun readsTheAnnouncementFromStatus() {
        assertNull(parse(""))
        assertNull(parse(""","announcement":null"""))
        assertEquals(
            Announcement("Update", "https://x.org", Announcement.Level.URGENT),
            parse(""","announcement":{"message":"Update","url":"https://x.org","level":"urgent"}"""),
        )
        assertEquals(
            Announcement("Hi", null, Announcement.Level.INFO),
            parse(""","announcement":{"message":"Hi","url":null,"level":"sparkly"}"""),
        )
    }
}
