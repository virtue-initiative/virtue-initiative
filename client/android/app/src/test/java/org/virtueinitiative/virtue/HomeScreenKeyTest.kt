package org.virtueinitiative.virtue

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test

// CLIENT-033: the main screen rebuilds when the announcement changes, with no
// restart, even when nothing else on it has changed.
class HomeScreenKeyTest {
    private fun key(status: String) =
        homeScreenKey("ACTIVE", JSONObject(status), permissionsGranted = false, readyVersion = null)

    @Test
    fun changesWhenTheAnnouncementChanges() {
        val base = """{"is_authenticated":true,"last_screenshot_at_ms":1000,"partner_count":1"""
        val none = key("$base}")
        val urgent = key("""$base,"announcement":{"message":"Update","url":null,"level":"urgent"}}""")
        val warning = key("""$base,"announcement":{"message":"Update","url":null,"level":"warning"}}""")
        val reworded = key("""$base,"announcement":{"message":"Update now","url":null,"level":"urgent"}}""")

        assertNotEquals(none, urgent)
        assertNotEquals(urgent, warning)
        assertNotEquals(urgent, reworded)
        assertEquals(urgent, key("""$base,"announcement":{"message":"Update","url":null,"level":"urgent"}}"""))
    }
}
