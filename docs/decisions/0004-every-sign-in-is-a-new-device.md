# 0004: Every sign-in is a new device

**Status:** Accepted

## Context

Uninstalling Virtue can't be detected reliably. Most platforms give an app no notice before it
is removed, and a removed app can't report anything afterwards. Someone could uninstall,
reinstall later and sign in again, and if the new install carried on as the same device, the
gap would be easy to miss.

## Decision

Uninstalling is not detected. Instead, every sign-in registers a new device with the API
(CORE-008), and signing out deactivates the old one (CLIENT-005). Partners see one device stop
reporting and a new one appear, which is visible even when the uninstall itself wasn't.

## Consequences

- A person who reinstalls, or signs out and back in, gets a new entry in their device list.
  The sign-out confirmation says so.
- Events queued but not uploaded when the old session ends are discarded, so the new device
  shares no state with the old one.
- Partners have to notice a device going quiet. Their digest emails name devices with no logs
  in the period to make that easier.
