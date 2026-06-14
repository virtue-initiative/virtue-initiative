# Tampering Detection

Events (all but Ping should be forwarded to the uploader)

- ComputerShutdown
- ProcessKilled (by computer, kill [pid], systemd stop/restart, etc.)
- ProcessStarted
- ProcessStopped (by user `virtue daemon stop`)
- ComputerBooted
- ComputerSuspended
- ComputerResumed
- Ping

Alerts

- Med: ProcessKilled without ComputerShutdown or ProcessStopped
- High: ProcessStopped
- High: ProcessStarted with last Ping > 7000ms without (ComputerShutdown and ComputerBoot)
- High: last Ping > 7000s without (ComputerSuspend and ComputerResume)

Rules:

- Ping should always come after pending lifecycle events (i.e. the event loop shouldn't start until the lifecycle events are queued up)
  - This might need to be different for suspend and resume, they might just need a grace period
