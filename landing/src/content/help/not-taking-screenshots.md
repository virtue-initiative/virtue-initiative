---
title: "Virtue isn't taking screenshots"
description: 'Find out why screenshots stopped on a device, and how to get them going again.'
topic: troubleshooting
order: 2
---

Start with the Virtue app on the device. The line at the top says what state monitoring is in,
and most problems show up there.

## What the app says

| The app says                                                             | What to do                                                                               |
| ------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------- |
| **Signed out**                                                           | Sign in again. The device is added to your account as a new device.                      |
| **Monitoring stopped** or **Monitoring paused**                          | Start or resume monitoring from the app. On Linux, run `virtue daemon start`.            |
| **Screen Recording off** (Mac)                                           | Follow the steps in the app to turn on Screen Recording for Virtue.                      |
| **Accessibility is off** (Android)                                       | Follow the steps in the app to turn Accessibility back on for Virtue.                    |
| **Setup not finished** or **Safari permission needed** (iPhone and iPad) | Work through the setup steps the app shows. Virtue can only capture Safari.              |
| **Monitoring needs attention** (Windows)                                 | Read the message under it, then use **Report an Issue** in the app if it does not clear. |

On Linux, run `virtue status` to see the same information.

## Screenshots that are skipped on purpose

Virtue skips a screenshot when the screen has not changed since the last one, and when the screen
is locked or the screensaver is on. These are normal. **Status Details**, in the app's menu, shows
the last screenshot taken and the reason the last one was skipped.

## Take a test screenshot

Choose **Test Screenshot** in the app's menu. On Android, tap **Take Test Screenshot** under
**Advanced** in **Status Details**. On Linux, run `virtue test-screenshot`. Virtue waits 10
seconds so you can bring up the screen you want, then tells you whether the screenshot was
uploaded or why it was not. An uploaded test screenshot appears on your **Logs** page in the web
app.

On iPhone and iPad, tap **Capture Next Safari Page** under **Advanced** in **Status Details**,
then open a page in Safari. Virtue captures it and uploads it with your next batch.

## Still stuck

[Report the problem](/help/reporting-an-issue) from the Virtue app on that device. Reports sent
from the app include its recent logs, which helps us find the cause.
