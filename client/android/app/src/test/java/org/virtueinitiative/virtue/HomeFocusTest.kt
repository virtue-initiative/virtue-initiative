package org.virtueinitiative.virtue

import org.junit.Assert.assertEquals
import org.junit.Test

class HomeFocusTest {
    // CLIENT-030: the sign-in form is shown only once notifications are allowed.
    @Test
    fun signInWaitsForNotifications() {
        assertEquals(HomeFocus.ALLOW_NOTIFICATIONS, HomeFocus.of(accessibilityConnected = true, signedIn = false, notificationsEnabled = false))
        assertEquals(HomeFocus.SIGN_IN, HomeFocus.of(accessibilityConnected = true, signedIn = false, notificationsEnabled = true))
    }

    // CLIENT-030: Accessibility off shows the setup guide, and a signed-in user is never sent back to allow notifications.
    @Test
    fun guideAndStatusDoNotDependOnNotifications() {
        for (notifications in listOf(true, false)) {
            assertEquals(HomeFocus.SETUP_GUIDE, HomeFocus.of(accessibilityConnected = false, signedIn = false, notificationsEnabled = notifications))
            assertEquals(HomeFocus.SETUP_GUIDE, HomeFocus.of(accessibilityConnected = false, signedIn = true, notificationsEnabled = notifications))
            assertEquals(HomeFocus.STATUS, HomeFocus.of(accessibilityConnected = true, signedIn = true, notificationsEnabled = notifications))
        }
    }
}
