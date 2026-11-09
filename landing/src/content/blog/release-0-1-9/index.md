---
title: A cleaner look for the Virtue apps
description: Version 0.1.9 redesigns the Virtue apps on every platform and makes the partner email reports easier to read.
pubDate: 2026-11-08
author: The Virtue Initiative team
---

Virtue 0.1.9 is released! We cleaned up the interface for all our apps.
Previously, they had become a bit cluttered. They showed a lot at once: status
text, settings, and buttons for things people rarely need. The new design
focuses on making sure it is clear when the app is set up right, and it now
shows you the next step if it's not.

![The Windows app showing "Monitoring active"](./success-page-windows.png)

Everything else (status details, what Virtue monitors, reporting an issue,
checking for updates and signing out) is in a menu in the top right corner.

![The Mac app showing "Monitoring active"](./success-page-mac.png)

We followed the same pattern for the phone apps. Here is the same page on
Android and iPhone:

![The Android app showing "Monitoring active"](./success-page-android.png)

![The iPhone app showing "Monitoring active"](./success-page-ios.png)

## Easier to read email reports

If you are someone's accountability partner, you get a regular email report.
We've simplified the emails to focus on the important information.

![A daily email report with screenshot, high risk and medium risk counts](./digest-email.png)

## Behind the scenes

We also spent a lot of time on things you won't see. We fixed a few privacy
issues in how login and password reset work, and added a large set of automated
tests so we can make changes without breaking things. We are also testing a new
way of checking screenshots that looks at each image on the screen separately.
It is not turned on yet, but early results catch far more inappropriate content
without flagging ordinary pages.

## How to update

Most apps (should) update themselves. On Windows and Mac, you can also open the
menu and choose "Check for Updates".

As always, if you have questions, ideas or run into a problem, email us at
[help@virtueinitiative.org](mailto:help@virtueinitiative.org).
