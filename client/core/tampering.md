# Tampering Detection

**This document previously described a richer boot-vs-monotonic-clock
suspend/reboot model (`GapTracker`, three separate gap buckets
`UnexpectedGap`/`UnexpectedStart`/`UnexpectedStop`, `SuspendDetected`/
`SystemLogin`/`SystemLogout` log rows, per-bucket sliding-window budgets and
cooldowns). That model was retired in the single-sequential-daemon-loop
rewrite as a deliberate simplification, not replaced feature-for-feature —
see the rewrite plan's "Lifecycle model simplification" note. It no longer
reflects the code.**

The current, much simpler model is specified in `SPEC.md` §2 (the daemon
loop spec, kept minimal by design):

- Each tick compares the actual wakeup time to the wakeup time it was
  scheduled for. The difference is appended to a last-10 array
  (`LifecycleState.late_wakeups`), unless the wakeup is within 2 minutes of
  a system login, or was scheduled within 2 minutes of a system logout — in
  either case it's excused, not recorded.
- An alert (`UploadKind::ScreenshotMissed`, `HIGH_RISK_LIFECYCLE_ALERT`) fires
  whenever a single entry exceeds 1 minute, or the sum of the array's
  non-negative entries exceeds 5 minutes.
- `UserStop` (`EXTRA_HIGH_RISK`, immediate) is unrelated to the above — it's
  driven directly by an explicit user action (`Daemon::note_user_stop`,
  reached via `UserStopRequested` over IPC) and was preserved unchanged
  through the rewrite.

Implementation: `client/core/src/module/lifecycle.rs`.

## Known limits (accepted under the current threat model)

- **Quit-and-never-restart** produces no next tick, so it's invisible to
  this model locally. Caught only by heartbeat silence — a partner noticing
  logs stop arriving.
- **Client-stored state is trusted.** Not defending against local tampering
  yet; that's a separate follow-up.
- The simplified model no longer distinguishes _why_ a wakeup was late
  (crash vs. suspend vs. late boot vs. force-kill-before-logout) the way the
  retired model's three separate buckets did — it only measures lateness
  against the schedule. This is an intentional trade of detection nuance for
  a much smaller, easier-to-reason-about implementation.
