---
sidebar_position: 2
---

# Log types

Every entry in your log feed has a **type** and a matching icon. Most entries are
routine activity (screenshots, sign-ins). A few are **alerts** that flag an
interruption in monitoring and sometimes indicate a follow-up communication to
your partner may be needed.

Open any entry to see its details, the device it came from, and a link back to
this page.

## Screenshot

A screenshot was captured on the device. This is the most common entry. The
monitoring app captures the screen at random moments, about every five
minutes on average, while it is running.

## Screenshot Skipped

Monitoring was active but no screenshot was uploaded. Either the screen had
not changed since the last capture, or the screen was locked, showing the
screensaver, or turned off. This keeps the timeline continuous without storing
redundant images.

## System Login

The user logged into this computer, or the computer started up (on systems
without a separate login step).

## System Logout

The user logged out of this computer, or the computer was shut down.

## Screenshot Missed

A scheduled screenshot was noticeably late, or several recent ones added up to
a longer-than-expected delay, and it wasn't explained by a nearby sign-in,
sign-out, or the device being asleep. Time spent asleep does not create this
event. It can happen when the device was under heavy load, was offline, or
monitoring was interrupted. An occasional one is normal; frequent ones are
worth investigating.

## Monitoring Stopped by User

A user stopped the monitoring process intentionally. This event cannot happen
without user intervention.

## Monitoring Resumed by User

A user resumed monitoring after previously stopping it.

## Repeated Restarts

The monitoring process started many times in a short span (more than 20 times
within 10 minutes). This can indicate someone is repeatedly trying to disable
monitoring by killing the process, or a genuine crash loop. Updates can also
cause a few restarts, but not this many. Investigate if you see this.

## Alert

A general alert was raised. Open the entry to read the alert message.

## Capture Failed

Screenshot capture failed several times in a row on the device (5 failures
within 30 minutes). This usually points to a configuration or permissions problem with screen capture — see the
[installation guide](/download) for your platform.

## Developer

A developer or diagnostic log entry. These are produced by developer tooling and
are not part of normal monitoring.

## Daily Check-in

Once a day, your device sends a small update to confirm that monitoring is
still active, even when there's nothing else to report.
