# SPEC.md vs current code — review notes

`SPEC.md` is a brand-new file (not present on `staging`), so this was a full comparison against
`src/`, not a diff review. Every open item was walked through with the author; this is the
resolved result. Section numbers match the current `SPEC.md`.

## Spec edits made this session

- **§1.1** — Added the `DeviceSettings` type (previously referenced in §6.1/§6.2 but never
  defined), including a new `hash_token: HashServerToken` field.
- **§1.1** — `HashServerToken`'s `type` claim fixed from `"hash-server"` to `"device"` (matches
  `src/lib/jwt.ts`, `hash-server/SPEC.md`, and the CLAUDE.md-locked claim value — was a spec typo,
  not an intended rename).
- **§1.5 (new)** — Added a general status-code convention: 204 for no-content responses, 200
  otherwise (including resource creation). Almost every status-code question below resolved in
  favor of this rule, so it's now stated once instead of implied per-endpoint.
- **§2.8** — Heading fixed from `POST /email-verification` to `POST /email-verification/validate`
  (the basic-types entry in §1.1 already had the right path; only the section heading was wrong).
- **§4.1** — `GET /device`'s `status` enum extended to `"online" | "offline" | "logged_out"`
  (third state: soft-deleted via `POST /d/logout`, not yet hard-deleted).
- **§6.1** — `POST /d/device` response: `token` is now the `DeviceRefreshToken` (opaque,
  long-lived Bearer credential for `/d/*`); the JWT hash-server token moved into
  `DeviceSettings.hash_token`.
- **§6.4** — Added the previously-missing response shape for `POST /d/batch`:
  `{ id, start_time, end_time, end_hash, url, settings }` at `200` — no top-level `token`, since
  it now lives in `settings.hash_token`.

## Confirmed — code needs to change to match the spec

None of these need further spec changes; they're implementation work against `SPEC.md` as it now
reads.

| Endpoint                                                                                                                   | Change                                                                                                                                                                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `POST /signup-request`                                                                                                     | `200 {ok:true}` → `204` no body                                                                                                                                                                                                                                 |
| `POST /signup`                                                                                                             | `201` → `200`                                                                                                                                                                                                                                                   |
| `POST /login`                                                                                                              | `200 {ok:true}` → `204` no body                                                                                                                                                                                                                                 |
| `POST /partner`                                                                                                            | `201` → `200`                                                                                                                                                                                                                                                   |
| `PATCH /device/:id`                                                                                                        | `200 {id, updated:true}` → `204` no body                                                                                                                                                                                                                        |
| `POST /d/device`                                                                                                           | `201` → `200`                                                                                                                                                                                                                                                   |
| `POST /d/batch`                                                                                                            | `201` → `200`; response drops top-level `token` (now `settings.hash_token`); request body moves to the nested `metadata` JSON format (`access_keys` flat map, `event_counts`) — **also needs a matching change in `client/core/src/api.rs`**, not just this API |
| `DELETE /user`                                                                                                             | `confirm_email` moves from JSON body to the `?confirm_email=` query param                                                                                                                                                                                       |
| `GET /device`                                                                                                              | `status` gains a third value, `"logged_out"`                                                                                                                                                                                                                    |
| `GET /d/device`, `POST /d/device`, `POST /d/batch`                                                                         | `HashServerToken` moves from a top-level `token` field into `DeviceSettings.hash_token`                                                                                                                                                                         |
| `GET /data`                                                                                                                | response gains `user`, `watching`, `watchers` alongside `batches`; `device_id`/`user` query-param filtering is dropped — the bundled response replaces per-device/per-partner filtering entirely                                                                |
| Partner digest cadence                                                                                                     | removed entirely — no more per-partner `digest_cadence` (drop `PATCH /partner/watching/:id`, the `digest_cadence` field on `watching` entries, and whatever in `lib/scheduler.ts`/`lib/digest-schedule.ts` reads it)                                            |
| `GET /` health                                                                                                             | needs a real `commit` field in the response (currently only `name`/`version`/`status`) — spec already expects this, code needs to embed the deployed commit SHA                                                                                                 |
| `POST /login` (unverified path)                                                                                            | needs new behavior: mint an `email_verification`-purpose token and email it when login hits an unverified account (spec's §2.7 already says this should happen; today the code just returns `403` with no email sent)                                           |
| `POST /email-verification/validate`                                                                                        | needs to accept `purpose: "email_verification"` tokens too (not just `email_change`) — this is how the new login-triggered reverification flow gets consumed                                                                                                    |
| `PATCH /partner/watching/:id`, `DELETE /partner/watcher/:id`, `DELETE /partner/watching/:id`, `GET /.well-known/jwks.json` | confirmed removed from the API — drop the routes and whatever `web/` UI depends on them                                                                                                                                                                         |

## Resolved as non-issues

- **`partner_invite_token` on `POST /signup`** — `API.md` documents this field, but it was never
  actually implemented: `shared-web/types.ts`'s real `signupSchema` has no such field, and
  `auth.ts` never reads one. The client already holds the invite token from the invite URL and can
  call `POST /partner/accept` directly once it has a session after signup — no round-trip through
  `POST /signup` needed. `SPEC.md` not having this field is correct; `API.md` is the stale one
  here (out of scope for this pass, but worth a note next time `API.md` gets touched).
- **`PartnerInfo.digest_cadence`** — not a spec omission; per-partner digest cadence is being
  removed outright (see table above), so `PartnerInfo` correctly has no such field.
