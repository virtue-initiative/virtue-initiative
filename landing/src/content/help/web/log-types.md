---
sidebar_position: 2
---

# Log types

Every entry in your log feed has a **type** and a matching icon. Most entries are
routine activity (screenshots, sign-ins, sleep/wake). A few are **alerts** that
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

## Computer Started

The device was powered on or restarted.

## Sleep

The device went to sleep. No screenshots are captured while a device is asleep.

## Wake

The device woke from sleep and monitoring resumed.

## Signed In

A user signed in on the device.

## Signed Out

A user signed out of the device.

## Monitoring Started

The monitoring app started running on the device.

## Monitoring Stopped

Monitoring stopped on the device — for example, a user stopped the app.

## Computer Shut Down

The device shut down, which stopped monitoring.

## Screenshots Paused

Screenshot capture was paused on the device.

## Screenshots Resumed

Screenshot capture resumed on the device.

## Activity

A monitoring lifecycle event occurred that doesn't fall into the categories
above.

## Unexpected Gap

There was a gap in monitoring while the app was running — no events arrived for
longer than expected. This can happen when the device was offline, under heavy
load, or asleep without recording a sleep event. An occasional gap is normal;
frequent gaps are worth investigating.

## Process Stopped Unexpectedly

Monitoring was stopped before the device shut down normally. This can indicate
the app was closed or killed rather than the device shutting down cleanly.

## Process Force-Stopped

Monitoring was forcibly terminated before the device shut down.

## Monitoring Stopped by User

A user stopped the monitoring process.

## Unexpected Restart

The monitoring process restarted unexpectedly.

## Missing Wake Event

A wake event was expected after the device slept but never arrived.

## Alert

A general alert was raised. Open the entry to read the alert message.

## Capture Failed

Screenshot capture failed repeatedly on the device. This usually points to a
configuration or permissions problem with screen capture — see the
[installation guide](/help/installation) for your platform.

## Developer

A developer or diagnostic log entry. These are produced by developer tooling and
are not part of normal monitoring.
