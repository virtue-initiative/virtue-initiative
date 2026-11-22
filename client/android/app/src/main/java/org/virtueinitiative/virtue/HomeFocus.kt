package org.virtueinitiative.virtue

/** The one focus area Home shows (CLIENT-030). */
enum class HomeFocus {
    SETUP_GUIDE,
    ALLOW_NOTIFICATIONS,
    SIGN_IN,
    STATUS;

    companion object {
        fun of(accessibilityConnected: Boolean, signedIn: Boolean, notificationsEnabled: Boolean): HomeFocus = when {
            !accessibilityConnected -> SETUP_GUIDE
            signedIn -> STATUS
            // Nobody signs in without the "Monitoring active" notification.
            !notificationsEnabled -> ALLOW_NOTIFICATIONS
            else -> SIGN_IN
        }
    }
}
