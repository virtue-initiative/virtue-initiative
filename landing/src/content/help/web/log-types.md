---
sidebar_position: 2
---

# Log types

Every entry in your log feed has a **type** and a matching icon. Most entries are
routine activity (screenshots, sign-ins, suspend/resume). A few are **alerts** that
flag a gap or interruption in monitoring worth a second look.

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

## Suspend Detected

The device was asleep for a while. No screenshots are captured while a device
is asleep; this entry is logged retrospectively once monitoring resumes.

## Activity

A monitoring lifecycle event occurred that doesn't fall into the categories
above.

## Screenshot Missed

A scheduled screenshot was noticeably late, or several recent ones added up to
a longer-than-expected delay, and it wasn't explained by a nearby sign-in or
sign-out. This can happen when the device was offline, under heavy load, or
asleep without recording a sleep event. An occasional one is normal; frequent
ones are worth investigating.

## Unexpected Gap

There was a gap in monitoring while the app was running — no events arrived for
longer than expected. This can happen when the device was offline, under heavy
load, or asleep without recording a sleep event. An occasional gap is normal;
frequent gaps are worth investigating.

## Process Stopped Unexpectedly

Monitoring was stopped before the device's session ended normally. This can
indicate the app was closed or killed rather than the user logging out or the
device shutting down cleanly.

## Monitoring Stopped by User

A user stopped the monitoring process.

## Monitoring Resumed by User

A user resumed monitoring after previously stopping it.

## Unexpected Restart

The monitoring process restarted unexpectedly.

## Alert

A general alert was raised. Open the entry to read the alert message.

## Capture Failed

Screenshot capture failed repeatedly on the device. This usually points to a
configuration or permissions problem with screen capture — see the
[installation guide](/download) for your platform.

## Developer

A developer or diagnostic log entry. These are produced by developer tooling and
are not part of normal monitoring.
