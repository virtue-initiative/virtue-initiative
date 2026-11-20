# Accounts and keys

This page follows an account from signup to deletion and shows what happens to its keys at each
step, across the web app, the API and the client apps. In one sentence: every account has one key
pair, the private half only ever reaches the server encrypted with a key that comes from the
password, and devices only ever hold public keys. The exact rules are in the specs (each step
names its sections), and the primitives (Argon2id parameters, HKDF labels, HPKE suite) are laid
out in the developer help page
[`landing/src/content/help/developer/security.mdx`](../landing/src/content/help/developer/security.mdx).
This page is the map that connects them.

## The pieces

| Name                   | What it is                                                                         | Where it lives                                           |
| ---------------------- | ---------------------------------------------------------------------------------- | -------------------------------------------------------- |
| Password salt          | Random bytes chosen at signup, fed to Argon2id with the password                   | Server (`users`), returned by `GET /user/login-material` |
| `password_auth`        | `HKDF("auth", Argon2id(password))`, what the client sends instead of the password  | Sent on login; the server stores only its SHA-256        |
| Password wrapping key  | `HKDF("key", Argon2id(password))`, an AES key that encrypts the private key        | The browser's `localStorage` only                        |
| Key pair               | An X25519 key pair, one per account                                                | Public half on the server, in the clear                  |
| `encrypted_priv_key`   | The private key, encrypted with the password wrapping key                          | Server (`users`)                                         |
| Device `wrapping_keys` | The public keys a device encrypts batch keys for: the owner's and each partner's   | Device settings (API-002 `DeviceSettings`)               |
| Batch key              | A fresh AES-256 key for one batch                                                  | Only ever stored wrapped, in the batch's `access_keys`   |
| Web refresh token      | An opaque session token, valid for a year                                          | `refresh_token` cookie                                   |
| Device refresh token   | An opaque token that never expires in practice                                     | The device's `event_state.json`                          |
| Hash token             | A 2-hour JWT the device uses to post event hashes to the hash server (HASH- specs) | The device's `event_state.json`                          |

> Note: "wrapping key" means two different things in this repo. In the web app
> (`web/src/utils/api/session.ts`) it is the AES key that comes from the password and encrypts the
> private key. In the device settings and the Rust core it is the list of _public_ keys a device
> wraps each batch key for. They never meet, but the shared name is easy to trip on.

## Signing up

Signup happens only in the web app, because only the web app ever needs the private key. It
starts with `POST /signup-request` (API-009), which checks Turnstile and emails a link. The link
opens the signup page, which checks its token with `POST /signup/validate` (API-011). The browser
then asks `GET /user/login-material` (API-015) for the current Argon2id parameters, picks a random
salt, and runs the derivation in `derivePasswordMaterial` (`web/src/utils/api/crypto.ts`) to get
both `password_auth` and the password wrapping key. It generates the key pair, encrypts the private
key with the wrapping key, and sends everything except the password and the plain private key to
`POST /signup` (API-010). The server hashes `password_auth` once more with SHA-256
(`api/src/lib/password.ts`) and stores it with the rest.

`Session.fromFinishSignup` in `web/src/utils/api/session.ts` does this half. It also saves the
password wrapping key in `localStorage`, so the next page load can decrypt the private key again
without asking for the password.

## Signing in to the web app

Login runs the same derivation with the account's own salt. The browser asks
`GET /user/login-material?email=…` for the salt (the server makes up a stable fake one for an
email it does not know, so the response does not reveal whether an account exists), derives
`password_auth` and the wrapping key, and sends `POST /login` (API-016). The server sets the
`refresh_token` cookie. The browser then saves the wrapping key, fetches `GET /user`, and decrypts
`encrypted_priv_key` with it. On a later page load `Session.restore` repeats the last two steps
from the saved wrapping key, so the password is only typed once per browser. Logging out clears
the saved key and the log cache (WEB-013).

## Signing in on a device

A device signs in with the same email and password, and derives `password_auth` the same way in
Rust (`derive_password_auth` in `client/core/src/crypto.rs`, contract 4 in
[`contracts.md`](contracts.md)). It sends that to `POST /d/device` (API-035), which checks it like
a login and returns a device refresh token and the device settings. A device never derives the
password wrapping key and never sees the private key. It only needs to _encrypt_, and for that the
public keys in `wrapping_keys` are enough.

Every sign-in registers a new device, even on a machine that was signed in before (CORE-008,
decision [0004](decisions/0004-every-sign-in-is-a-new-device.md)). Signing out with
`POST /d/logout` (API-037) revokes the token, deactivates the device and emails the owner and
their partners.

## Adding and removing partners

A partner invite is a link, not a key exchange. `POST /partner` (API-023) emails the invite, the
invited person opens it and the web app checks it with `POST /partner/validate` (API-024), and
once they are signed in (or have signed up) `POST /partner/accept` (API-025) makes the partnership
`accepted`. No keys move at any point: the partner's public key was already on the server from
their own signup.

What changes is the device settings. The API builds `wrapping_keys` from the owner's public key
plus the public key of every accepted partner (`listBatchAccessRecipientsForOwner` in
`api/src/lib/db.ts`), and every `GET /d/device` and `POST /d/batch` response carries fresh
settings. So a new partner can read batches from the next upload after the device next hears from
the server (usually one batch later), and nothing uploaded before that. Removing a partner with
`DELETE /partner/:id` (API-027) works the same way in reverse: later batches are not wrapped for
them, and `GET /data` (API-033) stops listing the owner's batches for them at once, even though
older batches still carry a wrapped key with their name on it.

## Changing a password versus resetting it

These two sound alike and behave very differently, and the difference is the most important thing
on this page. Imagine the private key is a letter opener kept in a small safe, and the password is
the combination. The server holds the safe (that is `encrypted_priv_key`) but has never known the
combination. When you _change_ your password, you still know the old combination, so you open the
safe, take the letter opener out and put it in a new safe with a new combination. It is the same
letter opener, so every letter you were ever sent still opens. When you _reset_ a forgotten
password, nobody can open the old safe, so the only way forward is a brand-new letter opener in a
brand-new safe. The old letters were sealed for the old letter opener, and they stay sealed.

In the code, a change is `POST /user/password` (API-050): the browser decrypts the private key
with the current password, encrypts the same key with a key from the new password, and sends it
with a new salt and `password_auth`. The public key does not change. The server signs out every
other web session but leaves devices alone, since a device's token has nothing to do with the
password. A reset is `POST /password-reset` through `POST /password-reset/finalize`
(API-012 to API-014): the browser generates a new key pair and the server replaces the old one.
Batches wrapped for the old public key are lost to the owner, which is why the web app warns before
finishing a reset and points anyone who still knows their password to Settings instead (WEB-032).

The analogy breaks down in one helpful place. A batch is wrapped once for each reader, so a reset
loses the owner's own copy of old logs but **not** their partners' copies. Their partners can
still read everything uploaded before the reset, and accountability carries on.

## Deleting things

Deleting a device from the web app (`DELETE /device/:id`, API-031) deletes it and its batches and
emails the owner and their partners. Deleting an account (`DELETE /user`, API-021) removes the
account, its devices, batches, sessions and tokens. In both cases the encrypted batch files in R2
are left to expire on the bucket's 30-day rule rather than deleted one by one (see
[`api/architecture.md`](../api/architecture.md)). Without the database rows nothing can find
them, and without a private key nothing could read them anyway.

## Where the code is

| Step                         | Web                                                       | API                                                      | Rust core                        |
| ---------------------------- | --------------------------------------------------------- | -------------------------------------------------------- | -------------------------------- |
| Deriving keys from passwords | `derivePasswordMaterial` in `web/src/utils/api/crypto.ts` | `api/src/lib/password.ts`                                | `derive_password_auth`           |
| Sessions and saved keys      | `web/src/utils/api/session.ts`                            | `api/src/routes/auth.ts`, `api/src/middleware/auth.ts`   | `client/core/src/module/auth.rs` |
| Device settings              |                                                           | `buildDeviceSettings` in `api/src/routes/device-only.ts` | `client/core/src/api.rs`         |
| Partners                     | `web/src/pages/Partners/`                                 | `api/src/routes/partners.ts`                             |                                  |
| Wrapping and unwrapping      | `unwrapBatchKey` in `web/src/utils/api/crypto.ts`         |                                                          | `client/core/src/crypto.rs`      |

This page covers the happy paths. There is more to each step (rate limits, bounced emails,
Turnstile, what happens when an invite is withdrawn), and the specs cover those. If something
here disagrees with a spec, the spec wins, and this page should be fixed.
