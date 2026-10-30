# Web App

## WEB-001 Overview

The web app is where people read what their devices recorded. A **monitored person** signs up, installs Virtue on their devices, and invites **partners** by email. Each partner who accepts can read that person's logs and reports. One account is often both: it monitors its own devices and watches other people. Every batch is decrypted in the browser (BATCH-001), so the server never sees log contents. The app MUST present only what the signed-in account is allowed to read, and MUST make clear when what it shows is incomplete, unverified or unreadable.

This spec defines what a user can see and do. Sections WEB-002 to WEB-027 apply to every page; the sections after them describe each page. Server behavior is defined in `api/SPEC.md` and is referenced by ID rather than restated.

## WEB-002 Pages and navigation

A signed-in user MUST be able to reach these pages:

| Path                                                         | Page                         |
| ------------------------------------------------------------ | ---------------------------- |
| `/`                                                          | Devices (WEB-034)            |
| `/partners`                                                  | Partners (WEB-049)           |
| `/passwords`                                                 | Passwords (WEB-054)          |
| `/report`, `/report/:userId`                                 | Report (WEB-045)             |
| `/logs`, `/logs/list`, `/logs/:userId`, `/logs/:userId/list` | Logs (WEB-038)               |
| `/settings`                                                  | Settings (WEB-058)           |
| `/admin`                                                     | Admin (WEB-065)              |
| `/invite-accept?partner_token=…`                             | Invite acceptance (WEB-053)  |
| `/verify-email?token=…`                                      | Email verification (WEB-033) |

Each page MUST have a heading naming it. `/logs/gallery` and `/logs/:userId/gallery` MUST keep working as aliases of the gallery view, so old links still open. Any other path MUST show the 404 page (WEB-070).

Signed-out routing is defined in WEB-005.

### WEB-003 Sidebar

Every signed-in page MUST show the sidebar. On narrow screens it MUST collapse into a menu that closes after navigating.

- **Devices**, **Partners** and **Passwords** links MUST show a count when it is above zero: the user's own devices, accepted partners in both directions, and locked passwords that are not deleted.
- Clicking **Devices** or **Partners** MUST refresh that page's data (WEB-012), even when already on it.
- **Reports** MUST list "My report" and one "<name> report" per accepted watched person. **Logs** MUST list "My logs" and one "<name> logs" per accepted watched person. `<name>` is the person's name, or their email when they have none. The link for the current page MUST be marked active.
- The account button MUST show the user's name, else their email, else "Account". It MUST open a menu with **Settings**, **Help** (opens the help site in a new tab), **Report an issue** (WEB-064) and **Log out** (WEB-007).
- There MUST NOT be a link to Admin (WEB-026).

## WEB-004 Session and authentication

A session is opened by logging in (WEB-029), finishing sign-up (WEB-031) or resetting a password (WEB-032). The app derives the password material as in API-015 and API-016, and keeps the key that unlocks the user's private key in the browser so that a reload stays signed in. While the app checks for a saved session at startup, it MUST show only "Loading…".

> OPEN: startup treats any failure of `GET /user` (API-019), including being offline, as signed out and shows the login page — should a network failure at startup show an error and keep the session instead?

### WEB-005 Signed-out access and return-to

A signed-out user MUST be able to open `/login`, `/signup`, `/forgot-password` and `/verify-email`. Any other path MUST redirect to `/login?to=<path and query>`.

- After logging in or finishing sign-up, the app MUST go to the `to` path when present, and otherwise to `/`.
- Switching between **Log in**, **Sign up** and **Forgot your password?** MUST keep `to`. A sign-up request MUST pass `to` to the server so that the emailed link returns there (API-009).
- A signed-in user who opens `/login`, `/signup` or `/forgot-password` MUST be redirected to `/`.

> OPEN: the redirect for signed-in users also swallows emailed sign-up links (`/signup?signup_token=…`) and reset links (`/forgot-password?token=…`), with no message — should those links instead say that another account is signed in?

### WEB-006 Expired or invalid session

When any request is answered with HTTP 401 (API-003), the app MUST end the session as if the user had logged out (WEB-007), so the user lands on the login page with `to` set to the page they were on. It MUST do this once, however many requests fail.

> OPEN: the login page does not say why the user was sent there — should it show "Your session is invalid or expired. Please log in again."?

### WEB-007 Logging out

Logging out MUST call API-018, MUST clear this browser's log cache (WEB-013) and the saved key, and MUST then show the login page. A failure of API-018 MUST NOT stop the local logout.

> OPEN: the login page after a manual logout carries `to=<the page the user was on>`, so the next account to log in on that browser lands on the previous account's page (for example another person's logs) — should a manual logout drop `to`?

### WEB-008 Messages across a reload

Some actions end on another page. Their result MUST be shown as a toast on the next page: "Password reset successfully." after WEB-032 and "Your account has been deleted." after WEB-063. Each message MUST be shown once.

## WEB-009 Loading, empty and error states

Every view that loads data MUST distinguish three states: loading ("Loading…"), loaded and empty (a short message saying there is nothing, never a blank area), and loaded with content. It MUST NOT show the empty state while still loading.

> OPEN: when a list request fails with an HTTP error, Devices, Passwords, Settings and the Admin snapshot stay on "Loading…" forever with no message; a network failure shows only the toast from WEB-010 — should a failed load show an error with a way to retry?

### WEB-010 Error messages

- When the API returns an error, the app MUST show the server's `error` text. A 400 MUST read "Invalid request: <first validation message>", or "Invalid request data" when there is none. A 401 MUST read "Your session is invalid or expired. Please log in again." and a 404 "Requested resource was not found."
- When a request cannot reach the network, the app MUST show one toast, "Error: Couldn't connect to the network. Try reloading.", that stays until dismissed. It MUST NOT show it again within 3 seconds, and the action that failed MUST NOT add a second message of its own.
- When an action fails with no message of its own, the app MUST show the action's fallback message (for example "Failed to save").
- Form errors MUST appear inside the form or dialog they belong to; failures of single-button actions MUST appear as a toast.

## WEB-011 Caching and refresh

### WEB-012 Account lists

The user, devices, partners and locked passwords MUST each be fetched once per session and shared by every page, and MUST be fetched again after any change the user makes to them.

- Opening Devices or Partners MUST refresh their lists. While a refresh runs, the page MUST keep showing the previous list rather than a loading state.
- Accepting an invite, removing a partner or leaving a partnership MUST also refresh the device list, since it decides whose devices the user can see.
- Requests for the same list made at the same time MUST share one request.

> OPEN: Passwords is not refreshed on open, unlike Devices and Partners — should it be?

### WEB-013 Log cache

Decrypted logs are kept in a cache in this browser so that they load quickly. Logs, Report and the Devices page's last upload times (WEB-035) MUST read from it.

- A view MUST first show what is cached, then sync new batches from API-033 and decrypt them, and MUST update as logs arrive (WEB-040).
- A view MUST NOT ask the server again if the same person's batches were synced within the last minute, unless the user asked for fresh data (Retry, or a new session).
- The cache MUST hold at most 30 days of events, and MUST NOT decrypt batches uploaded more than 30 days ago.
- Cached data MUST be kept per signed-in account. A view MUST NOT show events cached for another account, or show one person's events under another person, even when they share a browser. Logout and session expiry MUST empty the cache.
- Deleting a device (WEB-037) MUST remove its events from the cache.
- **Clear cache** (WEB-062, and in Decryption stats) MUST delete the cache in every open tab and reload them all. It MUST NOT ask for confirmation, because nothing on the server is lost: logs are downloaded and decrypted again on the next view.
- A sync requested while another runs for the same person MUST be folded into it, unless the running one cannot stand in for it: it may skip the server as fresh when this one must not, it belongs to an earlier session, or it started before the private key was available. The person MUST then sync again once it ends.
- A batch whose owner cannot be determined MUST NOT be shown under anyone.
- Syncs for several people started together SHOULD share one request. A person synced from scratch (for example a newly accepted partner) SHOULD get a request of their own, so the others need not download 30 days again. A failure to sync one person MUST NOT fail syncing another.

### WEB-014 Several open tabs

Logs MUST keep loading when the app is open in several tabs, including when the tab holding the cache is closed, frozen by the browser or stops answering: another tab MUST take over and finish any request still waiting. A tab MUST NOT take over from a tab that is still answering, even after sitting idle. A log query that gets no answer for 45 seconds MUST end with an error (WEB-040) rather than load forever.

### WEB-015 Cache upgrades

A cache written by an older or newer release whose layout does not match MUST be discarded and rebuilt, rather than making views fail. A brand-new cache, or one with extra tables or columns, MUST NOT be treated as mismatched.

## WEB-016 Decryption and verification

The app MUST decrypt and verify batches as defined in BATCH-002 to BATCH-006, opening each batch key with the user's private key (BATCH-005). It MUST show events of unknown types (BATCH-003, WEB-041).

> OPEN: when the saved key cannot unlock the user's private key, nothing is decrypted, yet Logs says "Logs synced" and shows no warning — should the app say that logs can't be decrypted in this browser?

### WEB-017 Verified and unverified events

Events from a batch whose hash chain does not match (BATCH-006) MUST still be shown, and MUST be marked "⚠ Unverified" in the list view and the detail dialog.

> OPEN: the gallery marks an unverified event only with a red outline, the same outline used for High and Alert concern — should it also say "Unverified"?

### WEB-018 Batches that cannot be decrypted

A batch that fails to decrypt, decompress or decode MUST be recorded as failed with its error message, MUST NOT contribute any events, and MUST NOT be tried again until the cache is cleared (BATCH-002). A batch that fails because of the network or a timeout (20 seconds per batch) MUST be tried again on the next sync.

> OPEN: an event inside a decrypted batch that is not a byte string, or not valid msgpack, is treated as a network-style failure and retried on every sync, rather than failing the batch permanently as BATCH-002 requires — should it count as a permanent failure?

**Decryption stats**, opened from the info button on the Logs status line, MUST show for "All time, all devices" and for "Current filter": batches decrypted out of the total, batches that failed to decrypt, total events and total screenshots. When there are failures, the user MUST be able to expand a list of each distinct error with its count and a "Why?" link to the decryption errors help page. The dialog MUST offer **Clear cache** (WEB-013).

### WEB-019 Device status

Wherever a device is listed, its status from API-029 MUST be shown as a tag: "Online" (green), "Offline" (gray) or "Deactivated" (red, for `logged_out`). The tag MUST link to that status's section of the device status help page, in a new tab.

## WEB-020 Destructive actions

An action that deletes data or ends a relationship MUST ask for confirmation in a dialog that names what is affected, with **Cancel** and a danger-styled confirm button. This covers deleting a device, removing a partner or pending invite, revealing a locked password for the first time, deleting or permanently deleting a locked password, deleting the account, and deleting a feature flag.

- While the action runs, its buttons MUST be disabled and the confirm button MUST say what is happening (for example "Deleting…").
- If it fails, the app MUST show the error and MUST leave everything as it was. The item MUST NOT disappear until the server confirms.
- Restoring a locked password, clearing the cache and removing a feature flag override MUST NOT ask for confirmation.

## WEB-021 Forms and new passwords

A submit button MUST be disabled while its form is submitting, and MUST say so ("Saving…", "Sending…", "Please wait…"). Required fields MUST be marked required so the browser blocks an empty submit.

Wherever the user chooses a new password (WEB-031, WEB-032, WEB-061):

- It MUST be at least 12 characters. The field MUST say "Use at least 12 characters." as help text, and as an error once something shorter has been typed. A shorter password MUST NOT be submitted.
- Once it is long enough, the app MUST check it against the Have I Been Pwned range API, sending only the first five characters of its SHA-1 hash. If it has been seen, the app MUST warn "This password has appeared in N known data breaches. Choose a different one." with a link to haveibeenpwned.com. The warning MUST NOT block submitting, and a failed check MUST show nothing.
- The confirmation MUST match, or the form MUST show "Passwords do not match".

Every password field MUST have a show/hide toggle.

## WEB-022 Display conventions

### WEB-023 Times

- Times MUST be shown relative to now in prose ("3 hours ago"), and as "Never" when there is no time.
- Dense rows (a partner's device list) MUST use the compact form: "now" under a minute, then "25m ago", "3h ago", "2d ago", "6w ago", "1y ago".
- Exact dates and times MUST use the browser's locale.

> OPEN: helpers for day headings ("Today", "Yesterday", full date) and YYYY-MM-DD local date keys are tested, but no page uses them — should they be removed or used?

### WEB-024 Concern levels

An event's concern score (BATCH-003 `risk`) MUST be shown as a level, judged on the score rounded to a whole percentage: Low below 40%, Medium 40% to 69%, High 70% to 89%, Alert 90% and above. Badges MUST read "⚠ Alert", "⚠ High" and "Med"; Low has no badge. The detail dialog MUST show "<percent>% concern (<Level>)", or "Concern unavailable" for an event with no score.

## WEB-025 API version

The app MUST call the API under the current version prefix (API-005).

> OPEN: the app does not handle HTTP 410 for an unsupported version; the user sees the bare error "Gone" on whatever action they tried — should it tell the user to reload for the new version?

## WEB-026 Admin-only access

`/admin` MUST NOT be linked from anywhere in the app. For a user who is not an admin (API-051 answers HTTP 403), `/admin` MUST show the 404 page and nothing of the Admin page.

> OPEN: until the first admin request is answered, a non-admin briefly sees the Admin heading and "Loading…", and the page's other admin requests are still sent — should the page wait for the answer before showing anything?

## WEB-027 Browser support

The app MUST work in browsers that lack native X25519 in Web Crypto or the native `Uint8Array` Base64 and hex methods, with the same results as browsers that have them (BATCH-005). Without them it MUST still read and write standard and URL-safe Base64, with or without padding, and hex, and MUST reject invalid input.

## WEB-028 Log in and sign up

The login, sign-up and password reset pages MUST show the product name, the line "Accountability starts here." and a note that Virtue is still being tested, with a link to report problems. The browser tab title MUST name the flow, for example "Log In to Virtue - The Virtue Initiative".

### WEB-029 Log in

- The page MUST show **Log in** / **Sign up** tabs, an email field, a password field, a **Log in** button and a "Forgot your password?" link.
- A failed login MUST show the server's error in an error alert on the page (API-016).
- On success the app MUST open the session and go to `to` or `/` (WEB-005).

### WEB-030 Sign up

- The page MUST ask only for an email, plus the Turnstile check when it is configured. **Send verification email** MUST stay disabled until Turnstile passes. Accepting the terms MUST NOT be required at this step.
- If Turnstile cannot load or errors, the page MUST say "The verification check could not load. Reload the page and try again." Each attempt MUST get a fresh Turnstile check, since a token works once.
- On success the app MUST open a "Check your email" dialog naming the address (API-009). Errors, such as a bounced address or a failed check, MUST show the server's message.
- The page MUST explain that sign-up creates an end-to-end encryption key for the account.

### WEB-031 Finish sign-up

The emailed link opens `/signup?signup_token=…`.

- The app MUST validate the token (API-011) and show the account's email, read-only. If the token is invalid, it MUST show the error and disable the password fields and checkboxes.
- The form MUST ask for an optional name, a password and its confirmation (WEB-021).
- **Create account** MUST stay disabled until the user ticks agreement to the Terms of Use and Privacy Policy, both linked.
- A newsletter checkbox MUST start unticked and MUST NOT block submitting; the opt-in MUST be sent only when it is ticked (API-010).
- On success the user MUST be signed in and taken to `to` or `/`. The account's timezone MUST be the browser's, or UTC.

### WEB-032 Forgot and reset password

- `/forgot-password` MUST explain that a reset makes logs uploaded before it unreadable, and that a user who knows their password should change it in Settings instead. Submitting MUST always say "If that email exists, a reset link has been sent." (API-012).
- The emailed link opens `/forgot-password?token=…`. The app MUST validate it (API-013), show the email read-only, and warn that a reset creates a new encryption key pair, so earlier batches stay unreadable and devices should sign in again. An invalid token MUST show the error and disable the fields.
- On success (API-014) the app MUST sign the user in, go to `/`, and show "Password reset successfully." (WEB-008).
- Both pages MUST link "Back to log in".

### WEB-033 Email verification

`/verify-email?token=…` MUST work signed in or out. It MUST show "Verifying…", then either "Email updated" with "Your email address has been successfully updated." and a **Go to settings** button, or "Verification failed" with "This verification link is invalid or has expired." A link without a token MUST show the error.

> OPEN: the page says "Email updated" even when API-017 reports `purpose: "email_verification"` rather than an email change, and **Go to settings** sends a signed-out user to the login page — should the message depend on the purpose?

## WEB-034 Devices

The Devices page (`/`) MUST list the user's own devices, not those of people they watch, as cards. While loading it MUST show "Loading…". With no devices it MUST show "Add your first device" with the setup steps and links to the download page.

### WEB-035 Device card

Each card MUST show the device's name, status tag (WEB-019), platform, **Last upload** and **Last activity** (from `last_hash_at`, API-029).

- **Last upload** MUST be the end time of the device's newest batch, taken from the log cache after syncing (WEB-013). It MUST show "Never" for a device with no batches, and "—" while unknown: before the cache answers, or when it has never synced and cannot now.
- **View logs** MUST open Logs filtered to that device.

### WEB-036 Add device

**Add device** MUST open a dialog with the setup steps (download, install, log in on that device) and links to the download page. It MUST mention that the iPhone and iPad app is in TestFlight.

### WEB-037 Rename and delete

**Edit** MUST open a dialog to rename the device (a name is required) (API-030). A failed save MUST show a toast and keep the dialog open.

**Delete device permanently** MUST ask for confirmation, warning that it permanently removes the device's logs and uploads and that partners will be notified (WEB-020, API-031).

## WEB-038 Logs

Logs shows events for one person: the user (`/logs`, "My logs") or a watched person (`/logs/:userId`, "<name>'s logs"). The heading MUST add " — <device name>" when one device is selected, and MUST add the "Deactivated" tag when that device is logged out. While the device and partner lists load on a watched person's page, the heading MUST say "Loading…".

> OPEN: for a `:userId` the user does not watch, the heading shows the first 8 characters of the ID and the page simply finds no logs, whereas Report says the report isn't available — should Logs say the same?

### WEB-039 Filters

Filters MUST live in the URL, so a link or reload keeps them and the back button undoes them. A filter at its default MUST be left out of the URL.

- **Date range** (`range`): "Past day" (default), "Past week" or "Past month", always ending today.
- **Device** (`device_id`): "All devices" or one of the person's devices. A `device_id` that is not one of their devices MUST be dropped once the lists load.
- **Concern** (`risk`): All, Alert, High (High and Alert) or Medium (Medium and above), per WEB-024.
- **Type** (`type`): All, or one category from WEB-041.
- **Show skipped screenshots** (`show_skipped`): off by default. While off, "Screenshot Skipped" events MUST be hidden unless the Type filter is "Screenshot Skipped".

On narrow screens the same filters MUST be available from an **Edit Search** button. When the filters match nothing, the view MUST say "No logs found."

> OPEN: "today" is taken as the UTC date but the range bounds are local midnight, so east of UTC the morning's logs fall outside "Past day" until the UTC date changes — should the range use the local date, as Report does?

### WEB-040 Sync status

The line above the logs MUST show one of:

- "Syncing logs…", or "Syncing logs… N/M blocks" once the number of batches to decrypt is known. Logs MUST appear as they are decrypted, without waiting for the sync to end.
- "Logs synced" when the sync finished. If the person's devices have events waiting to upload, it MUST add " · N items pending upload", and, for a single device whose next upload can be estimated from the gap between its recent batches, " · expected <relative time>".
- An error, with a **Retry** button that syncs again, when the sync could not finish. The logs already cached MUST stay visible. The error MUST read "Couldn't reach the server. Showing saved logs." for a network failure or timeout, "Another Virtue tab is using saved logs. Close other Virtue tabs, then select Retry." when another tab holds the cache, and "Couldn't sync logs." otherwise.

### WEB-041 Log types

Each event MUST be shown with an icon (never emoji text), a category and a message naming the device. The device name MUST fall back to the first 8 characters of its ID. Categories MUST match the headings of the log types help page, and **Learn more** MUST link to the category's section there (the category lowercased, with each run of other characters as `-`).

| `type` (and `data`)  | Category                   | Message                                                                                                     |
| -------------------- | -------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `screenshot`         | Screenshot                 | Screenshot captured on D                                                                                    |
| `screenshot_skipped` | Screenshot Skipped         | "Duplicate screenshot skipped on D", "Screen locked, screenshot skipped on D", or "Screenshot skipped on D" |
| `screenshot_missed`  | Screenshot Missed          | D missed a scheduled screenshot                                                                             |
| `system_login`       | System Login               | D was logged into or started up                                                                             |
| `system_logout`      | System Logout              | D was logged out of or shut down                                                                            |
| `user_stop`          | Monitoring Stopped by User | Monitoring stopped by user on D                                                                             |
| `user_start`         | Monitoring Resumed by User | Monitoring resumed by user on D                                                                             |
| `repeated_restarts`  | Repeated Restarts          | D restarted N times in a short window                                                                       |
| `alert`              | Alert                      | the alert's `message`                                                                                       |
| `capture_failed`     | Capture Failed             | Capture failed repeatedly on D                                                                              |
| `dev`                | Developer                  | `title: details`                                                                                            |
| `heartbeat`          | Daily Check-in             | an explanation of the daily check-in                                                                        |
| anything else        | the type, `_` as spaces    | Event on D                                                                                                  |

Events stored by older clients MUST still be shown: `lifecycle` with `kind` `system_login`, `system_logout` or `suspend_detected` ("Suspend Detected", "D was asleep for N min"), and `lifecycle_alert` with `reason` `unexpected_gap` ("Unexpected Gap"), `unexpected_stop` ("Process Stopped Unexpectedly"), `unexpected_start` ("Unexpected Restart") or `user_stop`. Other `lifecycle` events MUST read "Activity" and other `lifecycle_alert` events "Alert".

> OPEN: the help page has no sections for the older categories (Suspend Detected, Unexpected Gap, and so on), so their **Learn more** links land at the top of the page — should the help page cover them?

### WEB-042 Gallery view

The gallery is the default view.

- Screenshots MUST be laid out in rows that fill the width, each at its own aspect ratio, read from the image. An image whose size cannot be read MUST be laid out as 16:9. Rows aim for about 140 px high and MUST NOT shrink below 60% of that. A very wide image MUST get a row to itself, and the last row MUST NOT be stretched.
- Screenshot tiles with Medium concern or above MUST show the concern badge. Events without a screenshot MUST appear as cards with their icon, category, message and relative time.
- A tile MUST outline events with High or Alert concern, or from an unverified batch.

### WEB-043 List view

`/list` shows one row per event, newest first: a thumbnail or icon, the category, the device, the concern badge, "⚠ Unverified" when applicable, the message and the relative time. The view switch MUST keep the current filters.

In both views a date label MUST show the date and time of the first visible event, and the page MUST stay responsive with thousands of events.

### WEB-044 Log detail dialog

Clicking an event MUST open a dialog showing its icon and category, its message, its date and time, its concern (WEB-024), "⚠ Unverified" when applicable, a link explaining concern scores, and either the screenshot or an enlarged icon.

- **Previous log** and **Next log** buttons, and the left and right arrow keys, MUST step through the events in the current view. At either end the button MUST be disabled but stay in place. The arrow keys MUST NOT step while the user is typing in a field or while **Advanced** is open.
- Stepping to an event without a screenshot MUST NOT keep showing the previous screenshot.
- **Advanced** MUST list the event's other data fields, and MUST be hidden when there are none.

## WEB-045 Report

The report summarises flagged activity for one person over one day or week: the user's own (`/report`, "My report") or a watched person's (`/report/:userId`, "<name>'s report"). For someone the user does not watch, it MUST say "This report isn't available. You can only see reports for people you monitor." While the partner list loads, the heading MUST say "Loading…".

### WEB-046 Period and date

- **Daily** (default) covers one local calendar day. **Weekly** covers the seven days ending on the chosen date. Both live in the URL (`period`, `date`).
- The date MUST default to today. A missing, malformed or future date MUST be treated as today.
- Previous and next buttons MUST move by a day or a week. **Next** MUST be disabled on the current period. The date button MUST open a date picker that cannot go past today.
- The heading MUST read "<Month Dayth, Year> activity report" or "Activity report for the week of <Month Dayth, Year>".
- For the current period the report MUST note that it is incomplete, with a link to the previous period's report.
- While syncing, it MUST say "Syncing logs." or "Syncing logs (N of M blocks). Counts may still go up."

> OPEN: Report ignores sync errors and presents a failed sync as a complete report, unlike Logs (WEB-040) — should it show the same error and Retry?

> OPEN: the cache holds 30 days (WEB-013), so a report for an older date says "No screen activity was recorded" — should older dates be blocked, or say the data is no longer available?

> OPEN: the incomplete-period note says "If your partner reached out about something…" even on a watched person's report — should it be worded for the viewer?

### WEB-047 Summary

- It MUST say how many devices are being monitored, counting the person's devices that are not deactivated ("You have 2 devices being monitored." or "<First name> has…").
- It MUST count High and Alert events as "high risk" and Medium events as "medium risk", split into alerts (any flagged event that is not a screenshot) and screenshots, for example "There was 1 high risk alert and 2 high risk screenshots today." and "There was also 1 medium risk alert or screenshot."
- When nothing was captured, counting screenshots and screenshots skipped because the screen had not changed but not those skipped because it was locked, it MUST say "No screen activity was recorded <when>." and explain that this is often expected. On a watched person's report it MUST suggest reaching out if they were known to be using a device. It MUST NOT show the all-clear in this case.
- When there was activity but nothing was flagged, it MUST say "Nothing concerning was flagged <when>." with the number of screenshots captured, and on a watched person's report suggest sending a note of encouragement.
- When something was flagged, it MUST end with guidance: for the user's own report, that partners see the same report; for a watched person's, to reach out without shaming.
- It MUST link to the Logs page for the same person and period length.

> OPEN: the "See every log in the Logs tab" link passes only the period length, so from a past report it opens the past day or week ending today rather than the report's date — should Logs accept a date?

### WEB-048 Flagged events

Flagged events MUST appear in up to four titled rows, each with a count and hidden when empty: "High risk alerts", "High risk screenshots", "Medium risk alerts", "Medium risk screenshots". Within a row, events MUST be ordered by concern, then newest first. Rows MUST scroll sideways, with back and forward buttons when they overflow.

Each alert card MUST show the category, concern badge, device, time and a plain explanation:

- `user_stop`: how many minutes or hours later monitoring was turned back on on that device; otherwise that it "has not been turned back on yet", or for a finished period that it "was not turned back on before the end of this report".
- `repeated_restarts`: the restart count and window, noting a crash loop or someone trying to stop it.
- `screenshot_missed`, `capture_failed`: what usually causes them.
- `alert`: its message.

Clicking a card or screenshot MUST open the detail dialog (WEB-044), stepping within that row.

## WEB-049 Partners

Partners has two sections, each listing pending invites before accepted partners. Opening the page MUST refresh partners and devices (WEB-012). A partner MUST be shown by name, or by email with no name, and the email MUST then also appear as a tooltip.

> OPEN: the page has no loading state, so it shows "You cannot monitor anyone yet." and "No one can monitor you yet." until the list arrives (WEB-009) — should it show "Loading…"?

### WEB-050 You monitor

People whose logs the user can read. Empty: "You cannot monitor anyone yet."

- An accepted card MUST list up to four of the person's devices under "Devices (last seen)", each with its compact last-seen time (WEB-023) and status tag. More than four MUST add "+N more devices", which opens a dialog listing all of them. With none it MUST say "No devices".
- Clicking a device MUST open that person's logs for that device. **View logs** MUST open all their logs.
- **Remove** MUST confirm, saying the partner will be notified (WEB-020, API-027).

### WEB-051 Monitor you

People who can read the user's logs. Empty: "No one can monitor you yet."

- An accepted card MUST show the partner's email and the date they were added, and a **Remove** button that confirms first.
- **Invite partner** MUST open a dialog explaining that a partner can see screenshots and logs uploaded after they accept, with an email field. Errors MUST appear under the field (API-023). If the server reports the invite email bounced, the dialog MUST close and a toast MUST say "Email to <email> bounced, so the invite was not sent. Use a different email address."

### WEB-052 Pending and failed invites

- A pending invite MUST be tagged "Pending" and offer only **Remove**, which confirms with "This will cancel the pending partner relationship."
- An invite whose email bounced MUST be tagged "Email invite failed" and offer **Resend invite**. It MUST open a dialog with the address pre-filled for correction; sending MUST replace the failed invite with the new one (API-023 `replace_id`).

### WEB-053 Invite acceptance

The emailed link opens `/invite-accept?partner_token=…`. A signed-out user MUST log in or sign up first and then return here (WEB-005).

- The page MUST show "Processing invite…", then accept the invite (API-024, API-025).
- On success it MUST say "Invite from <name or email> accepted. You can now view their monitoring data." If this is the user's first accepted partnership, it MUST ask how often to receive summary emails (Daily or Weekly, default Weekly); **Done** saves the choice and goes to `/`, and a failure MUST show an error and stay. Otherwise it MUST offer **Back to dashboard**.
- On failure it MUST show "Invite error" with the server's message, or "Missing invite token." for a link without one, and **Back to dashboard**.

> OPEN: when looking up the invite fails but accepting succeeds, the message reads "Invite from accepted" with no name — should it fall back to a generic message?

## WEB-054 Passwords

Locked passwords (API-043) hold secrets the user should rarely need, such as a Screen Time passcode. The page MUST explain that revealing one permanently flags it and notifies every partner watching the user. While loading it MUST show "Loading…"; with none it MUST say "No locked passwords".

### WEB-055 Add a password

**Add password** MUST open a dialog asking for a name (up to 100 characters) and a secret value (up to 150 characters). The value MUST be encrypted in the browser to the user's own public key before it is sent, so only the user can read it (API-044). If the account has no key pair, it MUST say "Your account is not set up for locked passwords yet."

### WEB-056 Reveal

Each card MUST show the name, when it was added, and, once revealed, an "Accessed" tag and when.

- The first **Reveal** MUST confirm, saying it permanently flags the password and emails every partner, and that this can't be undone. A password already accessed MUST reveal without asking.
- The revealed value MUST be shown with a copy button ("Copied to clipboard") and a **Hide** button (API-046).

### WEB-057 Delete and restore

**Delete** MUST confirm and move the password to "Recently deleted" for 7 days (API-047). That section MUST appear only when it has entries; each shows when it was deleted, **Restore** (API-048) and **Delete permanently**, which confirms that it can't be undone (API-049).

## WEB-058 Settings

While the user is loading, Settings MUST show "Loading…".

### WEB-059 Profile

The form MUST start with the user's current values. The display name and email frequency (None, Alerts only, Daily, Weekly) MUST save together with one **Save** button (API-020). **Save** MUST be disabled until something changed, and MUST read "Saved" after saving until the next edit.

> OPEN: a blank name counts as no change, so a user cannot remove their display name — intended?

### WEB-060 Email

The email field MUST start with the current address. Changing the email MUST use its own form and **Verify** button, enabled only when the address differs (case and spaces ignored). When verification is required, a dialog MUST say a link was sent to the new address and that the email changes once it is confirmed. If the new address has bounced before, the form MUST say so and send nothing.

> OPEN: when no verification is required, success is shown as "Saved" on the Profile form's button, not on the Email form — should the Email form confirm it?

### WEB-061 Password

The user MUST enter their current password and a new one twice (WEB-021). The form MUST explain that existing logs stay readable, other browsers are logged out, and devices stay signed in (API-050).

- The app MUST refuse a new password equal to the current one, and MUST check the current password in the browser before contacting the server, showing "Current password is incorrect." when it is wrong.
- It MUST keep the user's key pair, so that batches sealed before the change still decrypt afterwards, and this browser MUST stay signed in across a reload.
- It MUST refuse when the private key does not match the account's public key, and change nothing.
- On success it MUST clear the fields and say "Your password was changed."

### WEB-062 Local cache

Settings MUST explain the local cache and offer **Clear cache** (WEB-013).

### WEB-063 Delete account

**Delete account** MUST open a dialog that requires typing the account's email (case and spaces ignored) before the confirm button is enabled (API-021). On success the user MUST be logged out and see "Your account has been deleted." (WEB-008). On failure the dialog MUST show the error and stay open.

## WEB-064 Report an issue

The dialog MUST require a description and offer an optional contact email, noting that the account email is used when it is blank (API-042). It MUST point users to the device app for problems with logs or screenshots. On success it MUST close and thank the user in a toast.

> OPEN: user-facing copy breaks the copy style checklist in a few places: em dashes in this dialog's note and success toast and in the Logs heading separator, and "Passwords do not match" has no full stop on the auth pages but has one in Settings — should these be fixed?

## WEB-065 Admin

For admins only (WEB-026, API-051).

### WEB-066 Latest snapshot

The page MUST show the newest analytics snapshot (API-052) as tiles: Users, Active users, Active devices, Devices, Batches, Partners and Locked passwords, each with its detail line, including per-person ratios computed from the counts. It MUST say when the snapshot was computed. With none it MUST say "No snapshot yet. Refresh data to compute one now." **Refresh data** MUST compute a new snapshot (API-053) and show it at once.

### WEB-067 History

The History section MUST show 90 days of snapshots, one chart per variable with its own y-axis rounded up to 1, 2 or 5 times a power of ten. Variables MUST include each count, the derived ratios, and one "Devices on <platform>" per platform seen. Points MUST be placed by date, oldest first. Hovering a day MUST show every chart's value for it.

- The admin MUST be able to hide and show variables, all at once or one by one. The choice MUST be remembered in this browser. With all hidden it MUST say "Every variable is hidden. Pick some above."
- A **Table** view MUST show the same variables by day. Whole numbers MUST be shown with grouping ("9,001") and ratios with two decimals ("1.50").

### WEB-068 Queries

- The admin MUST be able to run a preset (API-054) or raw SQL (API-055) with a row limit (default 100). Results MUST show as a table titled with the rows returned and rows read, or "No rows."
- A truncated result MUST say "Showing the first N rows." A result that read 50,000 rows or more MUST warn about the cost.
- A query the server holds because it would scan a whole large table MUST show the tables and query plan, with **Run anyway** to run it with `allow_scan`.
- **Download CSV** MUST save the results as RFC 4180 CSV named after the query and date, and MUST prefix text cells that start with `=`, `+`, `-`, `@`, tab or carriage return with `'` so a spreadsheet does not run them.

> OPEN: the truncation notice uses the row limit currently typed, not the one the query ran with — should it use the latter?

### WEB-069 Feature flags

The section MUST list every flag (API-062) with its key, rollout ("Overrides only" at 0%, "Everyone" at 100%, otherwise "N% of users"), when it was updated, its description and its overrides.

- A new flag MUST take a key of 1 to 64 lowercase letters, digits and underscores, a whole rollout percentage from 0 to 100 and an optional description (API-063). A key that already exists MUST be refused.
- Each flag's rollout and description MUST be editable, with **Save** enabled only when changed.
- The admin MUST be able to set an override on or off by user email, and remove one (API-065).
- **Delete flag** MUST confirm that it turns off for everyone and loses its overrides (API-064).

## WEB-070 Page not found

An unknown path MUST show "Page not found" with an explanation and buttons to the dashboard and Settings.
