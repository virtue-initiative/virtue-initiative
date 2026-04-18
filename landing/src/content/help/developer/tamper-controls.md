# Tamper protection

## General behavior

* Low or medium risk logs are batched. Only high risk logs are sent individually.
* Pings are logged locally every 60 seconds to handle service stopping without a chance to write a stop marker to disk.

## Specific events handled

* Suspend / wake: Zero-risk logs, there will be no screenshots taken between suspend and wake.
* Shutdown / startup: Zero-risk logs, there will be no screenshots taken between shutdown and startup. These will be wrapped by primary service stop/start logs, also zero-risk because we detect it was part of a shutdown cycle.
* Service stopped inside app (e.g. `virtue daemon stop` on Linux): “Are you sure?” question, then high risk log and marker indicating it was a user event so another high risk log is not generated on startup.
* Service stopped outside app / Service killed with cleanup allowed (e.g. `systemctl --user stop virtue.service` /  `kill <service pid>` on Linux): 0.5 risk log, unknown source of stopping. Will have a stop marker which will be read on startup.
* Service killed forcefully (e.g. `kill -9 <service pid>`): No alert can be sent on service stopping. Ping timestamps determine alert level on startup.
* Service started inside / outside app (e.g. `virtue daemon start` / `systemctl --user start virtue.service` on Linux): If stop marker present, zero-risk log if within 10 seconds. If no marker, zero-risk log if within 70 seconds of the last ping. Otherwise sends a high risk log.
* Login: Zero-risk log.
* Logout: “Are you sure?” question, then high risk log.
