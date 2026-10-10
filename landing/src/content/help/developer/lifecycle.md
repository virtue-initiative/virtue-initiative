# Lifecycle events

Last Updated: 2026-10-10

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

## Other lifecycle events

| Event               | Risk                                       | When                                                   |
| ------------------- | ------------------------------------------ | ------------------------------------------------------ |
| `system_login`      | 0.0 (informational)                        | The last known system login time changed.              |
| `system_logout`     | 0.0 (informational)                        | The last known system logout time changed.             |
| `screenshot_missed` | `HIGH_RISK_LIFECYCLE_ALERT` (0.8, batched) | Late wakeups crossed the thresholds above.             |
| `repeated_restarts` | `HIGH_RISK_LIFECYCLE_ALERT` (0.8, batched) | More than 20 starts within 10 minutes (30 min repeat). |
| `user_stop`         | `EXTRA_HIGH_RISK` (0.9, immediate/emailed) | The user explicitly stopped monitoring.                |
| `user_start`        | 0.0 (informational)                        | The user resumed monitoring.                           |

`HIGH_RISK_LIFECYCLE_ALERT` is kept just below `EXTRA_HIGH_RISK`. The upload
module sends anything at or above `EXTRA_HIGH_RISK` immediately and emails
it, so non-urgent alerts ride the normal batch.

## Login and logout hooks

Login and logout times come from `LifecycleHooks` (`client/core/src/platform.rs`),
implemented once per platform. They can be missing. Android has no logout
time, and iOS does not report lifecycle events at all.
