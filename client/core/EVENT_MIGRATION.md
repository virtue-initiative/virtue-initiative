# Event Model Migration Guide

## What `record_lifecycle_observation` was

`MonitorService::record_lifecycle_observation(LifecycleObservation)` was the old API for platform code to report lifecycle events (service start/stop, session changes, power state changes, etc.). It wrote observations to a `lifecycle_observations.jsonl` append-only file and triggered upload logic synchronously.

This has been replaced by an event-driven model:

```rust
service.queue_event(Event::ProcessStarted);
service.run_event_loop_iter();
```

Events are queued into a channel and dispatched to observers (lifecycle, screenshot, upload, capture availability) asynchronously on `run_event_loop_iter()`. Observers produce further events (e.g. `Event::Upload`) which are also dispatched in the same pass.

## Available events

```rust
Event::ProcessStarted
Event::ProcessStopped(ProcessStoppedReason::Shutdown | User | Other)
Event::ComputerSuspended
Event::ComputerResumed
Event::UserSessionChanged(UserSessionState::LoggedIn | LoggedOut)
Event::CaptureAvailabilityChanged(CaptureAvailabilityState::Ready | Blocked)
Event::CaptureFailed
```

## Shutdown sequence

Replace `service.shutdown()` with:

```rust
service.queue_event(Event::ProcessStopped(ProcessStoppedReason::Shutdown));
let _ = service.run_event_loop_iter();
let _ = service.mark_stopped();
```

## Linux daemon.rs — reference implementation

See `client/linux/src/daemon.rs` for the complete migration. Key patterns:

- **Startup**: `queue_event(Event::ProcessStarted)` + `run_event_loop_iter()`
- **Loop iteration**: call `service.loop_iteration()`, then queue `CaptureAvailabilityChanged(Ready|Blocked)` + `run_event_loop_iter()`
- **Suspend/resume**: `queue_event(Event::ComputerSuspended|Resumed)` + `run_event_loop_iter()`
- **Shutdown**: queue `ProcessStopped(Shutdown)` + `run_event_loop_iter()` + `mark_stopped()`

## Pending migrations

The following platforms still call the old API and will not compile until migrated:

| Platform | File                                     | Pending items                                                  |
| -------- | ---------------------------------------- | -------------------------------------------------------------- |
| macOS    | `client/mac/src/daemon.rs`               | `service.shutdown()`, `service.record_lifecycle_observation()` |
| Windows  | `client/windows/src/resident_monitor.rs` | `service.shutdown()`, `service.record_lifecycle_observation()` |
| iOS      | `client/ios/rust/src/lib.rs`             | `service.record_lifecycle_observation()`                       |
| Android  | `client/android/rust/src/lib.rs`         | `service.record_lifecycle_observation()`                       |

For each platform, follow the Linux daemon.rs pattern above.
