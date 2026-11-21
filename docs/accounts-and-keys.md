# Accounts and keys

This page follows an account from signup to deletion and shows what happens to its keys at each
step, across the web app, the API and the client apps. Every account has one X25519 key pair. The
private key reaches the server encrypted with a key derived from the password, and devices are
given public keys only. The exact rules are in the specs (each step names its sections), and the
primitives (Argon2id parameters, HKDF labels, HPKE suite) are in the developer help page
[`landing/src/content/help/developer/security.mdx`](../landing/src/content/help/developer/security.mdx).
This page shows how the steps connect and where each one lives in the code.

## The pieces

| Name                   | What it is                                                                         | Where it lives                                           |
| ---------------------- | ---------------------------------------------------------------------------------- | -------------------------------------------------------- |
| Password salt          | Random bytes chosen at signup, fed to Argon2id with the password                   | Server (`users`), returned by `GET /user/login-material` |
| `password_auth`        | `HKDF("auth", Argon2id(password))`, which the client sends to authenticate         | Sent on login; the server stores its SHA-256             |
| Password wrapping key  | `HKDF("key", Argon2id(password))`, an AES key that encrypts the private key        | The browser's `localStorage`                             |
| Key pair               | An X25519 key pair, one per account                                                | Public key on the server, in the clear                   |
| `encrypted_priv_key`   | The private key, encrypted with the password wrapping key                          | Server (`users`)                                         |
| Device `wrapping_keys` | The public keys a device encrypts batch keys for: the owner's and each partner's   | Device settings (API-002 `DeviceSettings`)               |
| Batch key              | A fresh AES-256 key for one batch                                                  | Stored wrapped for each reader, in `access_keys`         |
| Web refresh token      | An opaque session token, valid for a year                                          | `refresh_token` cookie                                   |
| Device refresh token   | An opaque token with a 1,000-year lifetime                                         | The device's `event_state.json`                          |
| Hash token             | A 2-hour JWT the device uses to post event hashes to the hash server (HASH- specs) | The device's `event_state.json`                          |

> Note: "wrapping key" means two different things in this repo. In the web app
> (`web/src/utils/api/session.ts`) it is the AES key that comes from the password and encrypts the
> private key. In the device settings and the Rust core it is the list of public keys a device
> wraps each batch key for. Check which one a piece of code means before you change it.

## Signing up

Signup happens in the web app, which is the one place that needs the private key. It starts with
`POST /signup-request` (API-009), which checks Turnstile and emails a link. The link opens the
signup page, which checks its token with `POST /signup/validate` (API-011). The browser then asks
`GET /user/login-material` (API-015) for the current Argon2id parameters, picks a random salt, and
runs `derivePasswordMaterial` (`web/src/utils/api/crypto.ts`) to get both `password_auth` and the
password wrapping key. It generates the key pair, encrypts the private key with the wrapping key,
and sends `password_auth`, the salt, the public key and `encrypted_priv_key` to `POST /signup`
(API-010). The server hashes `password_auth` once more with SHA-256 (`api/src/lib/password.ts`)
and stores it with the rest.

`Session.fromFinishSignup` in `web/src/utils/api/session.ts` does the browser's half. It also
saves the password wrapping key in `localStorage`, so the next page load can decrypt the private
key again without asking for the password.

## Signing in to the web app

Login runs the same derivation with the account's own salt. The browser asks
`GET /user/login-material?email=…` for the salt (for an unknown email the server makes up a
stable salt, so the response looks the same whether or not the account exists), derives
`password_auth` and the wrapping key, and sends `POST /login` (API-016). The server sets the
`refresh_token` cookie. The browser then saves the wrapping key, fetches `GET /user`, and decrypts
`encrypted_priv_key` with it. On a later page load `Session.restore` repeats the last two steps
from the saved wrapping key, so the password is typed once per browser. Logging out clears the
saved key and the log cache (WEB-013).

## Signing in on a device

A device signs in with the same email and password, and derives `password_auth` the same way in
Rust (`derive_password_auth` in `client/core/src/crypto.rs`, contract 4 in
[`contracts.md`](contracts.md)). It sends that to `POST /d/device` (API-035), which checks it like
a login and returns a device refresh token and the device settings. Devices only ever encrypt, so
the public keys in `wrapping_keys` are all they get. The password wrapping key and the private key
stay in the browser.

Every sign-in registers a new device, even on a machine that was signed in before (CORE-008,
decision [0004](decisions/0004-every-sign-in-is-a-new-device.md)). Signing out with
`POST /d/logout` (API-037) revokes the token, deactivates the device and emails the owner and
their partners.

## Adding and removing partners

`POST /partner` (API-023) emails an invite link. The invited person opens it, the web app checks
it with `POST /partner/validate` (API-024), and once they are signed in (or have signed up)
`POST /partner/accept` (API-025) marks the partnership `accepted`. The partner's public key has
been on the server since their own signup, so accepting changes which keys the owner's devices are
given and nothing else.

The API builds `wrapping_keys` from the owner's public key plus the public key of every accepted
partner (`listBatchAccessRecipientsForOwner` in `api/src/lib/db.ts`), and every `GET /d/device`
and `POST /d/batch` response carries fresh settings. A new partner can therefore read batches from
the upload after the device next hears from the server (usually one batch later). Batches uploaded
before then were wrapped for the old set of readers. Removing a partner with
`DELETE /partner/:id` (API-027) works the same way in reverse: later batches leave them out, and
`GET /data` (API-033) stops listing the owner's batches for them at once, although older batches
still hold a key wrapped for them.

## Changing a password and resetting it

A password change keeps the key pair, and a reset replaces it. That decides whether the owner can
still read their old logs.

A change is `POST /user/password` (API-050). The browser decrypts the private key with the current
password, encrypts the same key under a key derived from the new password, and sends it with a new
salt and `password_auth`. The public key stays the same, so every batch stays readable. The server
signs out the user's other web sessions and leaves device sessions alone, since a device token has
no tie to the password.

A reset runs from `POST /password-reset` to `POST /password-reset/finalize` (API-012 to API-014).
The old private key can only be decrypted with the old password, so the browser generates a new
key pair and the server replaces the old one. The owner loses access to every batch wrapped for the
old public key, which is why the web app warns before a reset and points anyone who still knows
their password to Settings (WEB-032). Each batch key is wrapped separately for every reader, so the
partners' copies are unaffected, and partners can still read everything uploaded before the reset.

## Deleting things

Deleting a device from the web app (`DELETE /device/:id`, API-031) deletes it, its batch rows
and their encrypted files in R2, and emails the owner and their partners. Deleting an account
(`DELETE /user`, API-021) removes the account, its devices, batches, sessions and tokens, but
leaves the batch files in R2 to expire under the bucket's 30-day rule (see
[`api/architecture.md`](../api/architecture.md)). With the database rows gone, nothing points at
them in the meantime.

## Where the code is

| Step                         | Web                                                       | API                                                      | Rust core                        |
| ---------------------------- | --------------------------------------------------------- | -------------------------------------------------------- | -------------------------------- |
| Deriving keys from passwords | `derivePasswordMaterial` in `web/src/utils/api/crypto.ts` | `api/src/lib/password.ts`                                | `derive_password_auth`           |
| Sessions and saved keys      | `web/src/utils/api/session.ts`                            | `api/src/routes/auth.ts`, `api/src/middleware/auth.ts`   | `client/core/src/module/auth.rs` |
| Device settings              |                                                           | `buildDeviceSettings` in `api/src/routes/device-only.ts` | `client/core/src/api.rs`         |
| Partners                     | `web/src/pages/Partners/`                                 | `api/src/routes/partners.ts`                             |                                  |
| Wrapping and unwrapping      | `unwrapBatchKey` in `web/src/utils/api/crypto.ts`         |                                                          | `client/core/src/crypto.rs`      |

This page covers the main paths. Each step has more to it (rate limits, bounced emails, Turnstile,
withdrawn invites), and the specs cover those. Where this page and a spec disagree, the spec is
right and this page needs fixing.
