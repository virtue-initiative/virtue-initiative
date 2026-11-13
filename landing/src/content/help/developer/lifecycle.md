# Lifecycle events

Last Updated: 2026-11-12

The client's daemon loop (`client/core/src/module/lifecycle.rs`) detects
tampering by checking whether each tick ran when it was scheduled. See CORE-002
in `client/core/SPEC.md` for the normative rules.

## Late wakeups

Each tick compares the actual wakeup time to the time it was scheduled for.
The difference is appended to a list of the last 10 wakeups, unless the gap is
excused. An alert (`screenshot_missed`, risk 0.8) fires when a single entry is
more than 2 minutes late, or the non-negative entries sum to more than 5
minutes. The list is cleared after an alert so one delay is not reported twice.

A gap is excused when it is explained by one of these:

- **Login evidence:** the wakeup is within 2 minutes of the last system login.
- **Logout evidence:** the wakeup was scheduled within 2 minutes of the last
  system logout.
- **Suspend evidence:** the system was suspended for essentially the whole
  gap. This is measured by comparing the wall clock to a clock that does not
  advance while suspended. It can only add an excuse, never block one.
- **User stop:** the first tick after a user-initiated stop is skipped.

A login or logout signal that is available but outside its window blocks the
login/logout excuse. Because suspended time is excused, a device that was
asleep does not produce a `screenshot_missed` event.

## Restart gaps

The daemon stamps a last-alive time at the end of every tick, at least every
60 seconds while it waits, and when it stops. On start it measures the time
since that stamp, less 60 seconds, and treats it as one late wakeup. This
reports a stop and restart that fits between two scheduled wakeups. The same
evidence excuses it, with the start time compared to the last login and the
last-alive time compared to the last logout. No gap is measured after a
user-initiated stop.

## Clock changes

The wall clock was set back when less wall-clock time than suspend-safe time
passed since the previous tick, or when the daemon starts at a time earlier
than its last-alive stamp. A change of more than 5 seconds is recorded as a
late wakeup of that length, and no evidence excuses it. A change seen within
10 minutes of boot is not recorded, because that is the OS correcting its
clock. Either way the daemon takes a screenshot at once, flushes the pending
batch and drops every schedule stored against the old clock. The loop also
counts each wait on the suspend-safe clock, so a clock change cannot hold off
a tick.

## Other lifecycle events

| Event               | Risk                                       | When                                                                      |
| ------------------- | ------------------------------------------ | ------------------------------------------------------------------------- |
| `system_login`      | 0.0 (informational)                        | The last known system login time changed.                                 |
| `system_logout`     | 0.0 (informational)                        | The last known system logout time changed.                                |
| `screenshot_missed` | `HIGH_RISK_LIFECYCLE_ALERT` (0.8, batched) | Late wakeups, restart gaps or clock changes crossed the thresholds above. |
| `repeated_restarts` | `HIGH_RISK_LIFECYCLE_ALERT` (0.8, batched) | More than 20 starts within 10 minutes (30 min repeat).                    |
| `user_stop`         | `EXTRA_HIGH_RISK` (0.9, immediate/emailed) | The user explicitly stopped monitoring.                                   |
| `user_start`        | 0.0 (informational)                        | The user resumed monitoring.                                              |

`HIGH_RISK_LIFECYCLE_ALERT` is kept just below `EXTRA_HIGH_RISK`. The upload
module sends anything at or above `EXTRA_HIGH_RISK` immediately and emails
it, so non-urgent alerts ride the normal batch.

## Login and logout hooks

Login and logout times come from `LifecycleHooks` (`client/core/src/platform.rs`),
implemented once per platform. They can be missing. Android has no logout
time, and iOS does not report lifecycle events at all.
