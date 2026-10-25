package org.virtueinitiative.virtue

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class UpdateManifestTest {
    private val sha = "c890dc1f4c68567b2ccf7b0d16def763c4990d7d833fc75261a097005b92cc84"

    @Test
    fun parsesAValidManifest() {
        val manifest = UpdateManifest.parse(
            """{"version":"0.1.5","url":"https://example.org/v.apk","sha256":"${sha.uppercase()}"}"""
        )
        assertEquals(UpdateManifest("0.1.5", "https://example.org/v.apk", sha), manifest)
    }

    @Test
    fun treatsAnEmptyFeedAsNoUpdate() {
        assertNull(UpdateManifest.parse("{}"))
        assertNull(UpdateManifest.parse("not json"))
    }

    @Test
    fun rejectsBadFields() {
        assertNull(UpdateManifest.parse("""{"version":"0.1.5-dev","url":"https://e/v.apk","sha256":"$sha"}"""))
        assertNull(UpdateManifest.parse("""{"version":"0.1.5","url":"https://e/v.apk","sha256":"abc"}"""))
        assertNull(UpdateManifest.parse("""{"version":"0.1.5","url":"file:///v.apk","sha256":"$sha"}"""))
    }

    @Test
    fun allowsPlainHttpOnlyWhenAsked() {
        val json = """{"version":"0.1.5","url":"http://localhost/v.apk","sha256":"$sha"}"""
        assertNull(UpdateManifest.parse(json))
        assertEquals("http://localhost/v.apk", UpdateManifest.parse(json, allowInsecureUrl = true)?.url)
    }

    @Test
    fun comparesVersionsNumerically() {
        assertTrue(UpdateManifest.isNewer("0.1.10", "0.1.9"))
        assertTrue(UpdateManifest.isNewer("1.0", "0.9.9"))
        assertTrue(UpdateManifest.isNewer("0.1.4.1", "0.1.4"))
        assertFalse(UpdateManifest.isNewer("0.1.4", "0.1.4"))
        assertFalse(UpdateManifest.isNewer("0.1.4", "0.1.4.0"))
        assertFalse(UpdateManifest.isNewer("0.1.3", "0.1.4"))
        assertFalse(UpdateManifest.isNewer("garbage", "0.1.4"))
        assertFalse(UpdateManifest.isNewer("0.1.5", "garbage"))
    }
}
