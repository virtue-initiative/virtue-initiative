# Web app architecture

This page explains how the web app is put together, and spends most of its time on the local log
cache, because that is the part that is hard to understand from the code alone. In one sentence:
every log has to be decrypted in the browser, so the app decrypts each batch once and keeps the
result in a SQLite database in the browser's private file storage, which one tab owns and every
other tab asks. What the app must do is in [`SPEC.md`](SPEC.md) (`WEB-` IDs), and
[`AGENTS.md`](AGENTS.md) lists the rules that are easy to break.

## The shape of the app

The app is a Preact single-page app built with Vite. Each route is a folder under `src/pages/`
with an `index.tsx`, and logic worth testing on its own sits in a plain `.ts` file next to it
(`src/pages/Report/report-model.ts`, for example). Every API call goes through
`src/utils/api/api.ts`, whose types come from `shared-web/types.ts` so the API and the app cannot
quietly disagree about a response. `src/utils/api/session.ts` holds the signed-in user and their
keys (see [`docs/accounts-and-keys.md`](../docs/accounts-and-keys.md)), and
`src/utils/api/crypto.ts` and `src/utils/api/batch-materializer.ts` are the TypeScript half of
the contracts with the Rust client ([`docs/contracts.md`](../docs/contracts.md)). Most of that is
ordinary, but the cache in `src/utils/cache/` is not, so the rest of this page is about it.

## Why there is a cache

The server cannot read anyone's logs, so it cannot search them, count them or send a page of
them. All it can do is hand the browser a list of encrypted batches (`GET /data`, API-033), one
for roughly every hour a device was on, and the browser has to download each one from R2, unwrap
its key, decrypt it, decompress it, decode it and check its hash chain before it can show a single
event. A week of logs from two devices is a few hundred batches. Doing that on every page view
would be slow on a laptop and painful on a phone (and it would cost us an R2 read per batch each
time).

So the app does the work once. Each decrypted event goes into a SQLite table, each screenshot into
another, and the Logs, Report and Devices pages read from there (WEB-013). The cache keeps 30 days,
the same as the server keeps batches, and never downloads anything older.

## One database, many tabs

The cache lives in OPFS (the browser's private file storage), opened through SQLite's
"SAH pool" VFS, and that comes with a hard rule: only one worker in one tab can have the database
open at a time. But people open the app in several tabs, and every one of them needs logs.

Imagine a family with one shared notebook that only one person can hold at a time. Passing it
around for every question would be chaos, so they agree that one person holds it, and everyone
else calls out what they want looked up or written down. If the holder leaves the room, someone
else picks it up. If the holder falls asleep with it in their lap, the others wait a little,
decide they are not waking up, and take over. That is close to what the tabs do. The holder is
the **leader** tab: the one holding the `cache-leader` Web Lock, which starts the worker
(`src/utils/cache/worker.ts`) that owns the database. The other tabs are **followers**. They send
their requests over a `BroadcastChannel`, and the leader answers on the same channel
(`src/utils/cache/client.ts`). When the leader's tab closes, the browser releases the lock and the
next tab in line becomes leader and replays any requests that were still waiting. When the leader
goes quiet for long enough while a follower is waiting (Android freezes background tabs, and a
frozen tab keeps its lock), a follower steals the lock (WEB-014).

The analogy breaks down in one place, and it is the place that causes real bugs. You can take a
notebook out of a sleeping person's lap, but a browser does not give up a frozen tab's open file
handles. So a new leader can hold the lock and still be unable to open the database. The worker
retries for a few seconds, then gives up and reports `cache-locked`, which the Logs page shows as
"Another Virtue tab is using saved logs."

## How a sync works

A page asks for logs with a query (a user, maybe a device, a time range, some event types), and
the leader's worker answers it in steps:

1. It answers straight away from SQLite with whatever is cached, so the page has something to
   show.
2. It fetches new batch rows with `GET /data?since=…` and merges them into the `batches` table.
   One response covers every person the viewer can see, so syncs for several people at once share
   it. A person synced within the last minute is skipped unless the user asked for a refresh
   (`src/utils/cache/sync.ts`).
3. It takes every batch that is neither decrypted nor marked failed, newest first, and decrypts
   them 24 at a time: download from R2, unwrap the key (the browser's own X25519 if it has it,
   otherwise the `@hpke` library, see `src/utils/api/hpke-native.ts`), then decrypt, decompress,
   decode and verify (`decryptAndFlattenBatch` in `src/utils/api/batch-materializer.ts`). Each
   batch's events and images are written to SQLite as it finishes.
4. While that runs it sends small progress ticks every quarter second, and the newly decrypted
   logs every five seconds, so the page fills in as it goes. When it is done it sends the full
   result, which replaces everything sent before.

A batch that fails to decrypt or decode is recorded in `failed_batch_ids` with its error and is
not tried again until the cache is cleared, because trying again would fail the same way. A batch
that fails because of the network is simply left for the next sync (WEB-018). The Decryption stats
dialog on the Logs page reads those failures back out, which is the first place to look when
someone says their logs are missing.

## Keeping it right

The cache is shared by everyone who signs in on the browser, so every row carries a `viewer_id`,
and a query only ever reads rows for the signed-in viewer. Logging out, or a session expiring,
empties it (WEB-013). The private key reaches the worker as a `CryptoKey` in the
`setSession` message, and switching accounts bumps a session counter so a sync that started for the
previous person cannot mark the new person's data as fresh.

The tables are declared in `src/utils/cache/schema.ts`. Every table is created with
`CREATE TABLE IF NOT EXISTS`, which does nothing to a table that already exists, so a cache made
by an older release would otherwise keep its old columns forever. To stop that, opening the
database compares what is there with what is declared, and drops and rebuilds everything on a
mismatch (WEB-015). Bump `CACHE_SCHEMA_VERSION` whenever you change a table. Nothing is lost by a
rebuild, since every log can be downloaded and decrypted again.

**Clear cache** (WEB-062) is the bigger hammer. It terminates the worker in every tab, deletes the
OPFS directory outright, and reloads every tab. It deletes files instead of asking SQLite to empty
the tables because it has to work even when the worker is the thing that is stuck.

## Where to look

| What                               | File                                                      |
| ---------------------------------- | --------------------------------------------------------- |
| Leader election and tab messaging  | `src/utils/cache/client.ts`                               |
| The worker: SQLite, sync, decrypt  | `src/utils/cache/worker.ts`                               |
| Table definitions and drift checks | `src/utils/cache/schema.ts`                               |
| When a sync hits the network       | `src/utils/cache/sync.ts`                                 |
| Turning one batch into events      | `src/utils/api/batch-materializer.ts`                     |
| Decryption and hash verification   | `src/utils/api/crypto.ts`, `src/utils/api/hpke-native.ts` |

The tests for all of this are in the same folder. `client.test.ts` drives several fake tabs
through leader changes, and `decryption.test.ts` runs real batches through the worker's pipeline.
If you change how tabs hand over the cache, test it by hand too, with the app open in two tabs and
one of them closed or put in the background, because a browser's real timing is not something the
tests can fully copy.
