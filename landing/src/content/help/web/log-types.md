---
sidebar_position: 2
---

# Log types

Every entry in your log feed has a **type** and a matching icon. Most entries are
routine activity (screenshots, sign-ins). A few are **alerts** that flag an
interruption in monitoring worth a second look.

Open any entry to see its details, the device it came from, and a link back to
this page.

## Screenshot

A screenshot was captured on the device. This is the most common entry — the
monitoring app captures the screen on a regular interval while it is running.

## Screenshot Skipped

Monitoring was active but no screenshot was uploaded — either the screen had
not changed since the last capture, or the device was locked or asleep. This
keeps the timeline continuous without storing redundant images.

## System Login

The user logged into this computer, or the computer started up (on systems
without a separate login step)

## System Logout

The user logged out of this computer, or the computer was shut down

## Screenshot Missed

A scheduled screenshot was noticeably late, or several recent ones added up to
a longer-than-expected delay, and it wasn't explained by a nearby sign-in or
sign-out. This can happen when the device was offline, under heavy load, or
asleep without recording a sleep event. An occasional one is normal; frequent
ones are worth investigating.

## Monitoring Stopped by User

A user stopped the monitoring process.

## Monitoring Resumed by User

A user resumed monitoring after previously stopping it.

## Repeated Restarts

The monitoring process was started, stopped, or killed and relaunched an
unusually high number of times in a short span. This can indicate someone is
repeatedly trying to disable monitoring by killing the process, or a genuine
crash loop. Investigate if you see this.

## Repeated Restarts

The monitoring process was started, stopped, or killed and relaunched an
unusually high number of times in a short span. This can indicate someone is
repeatedly trying to disable monitoring by killing the process, or a genuine
crash loop. Investigate if you see this.

## Alert

A general alert was raised. Open the entry to read the alert message.

## Capture Failed

Screenshot capture failed repeatedly on the device. This usually points to a
configuration or permissions problem with screen capture — see the
[installation guide](/download) for your platform.

## Developer

A developer or diagnostic log entry. These are produced by developer tooling and
are not part of normal monitoring.

## Daily Check-in

Once a day, your device sends a small update to confirm that monitoring is
still active, even when there's nothing else to report.
