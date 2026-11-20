# Web app architecture

This page explains how the web app is put together, and spends most of its time on the local log
cache, which is the hardest part to follow from the code. Every log is decrypted in the browser, so
the app decrypts each batch once and keeps the result in a SQLite database in OPFS. One tab owns
that database and the other tabs send it their requests. What the app must do is in
[`SPEC.md`](SPEC.md) (`WEB-` IDs), and [`AGENTS.md`](AGENTS.md) lists the rules that are easy to
break.

## The shape of the app

The app is a Preact single-page app built with Vite. Each route is a folder under `src/pages/`
with an `index.tsx`, and logic worth testing on its own sits in a plain `.ts` file next to it
(`src/pages/Report/report-model.ts`, for example). Every API call goes through
`src/utils/api/api.ts`, whose types come from `shared-web/types.ts`, the same file the API's
response types come from. `src/utils/api/session.ts` holds the signed-in user and their
keys (see [`docs/accounts-and-keys.md`](../docs/accounts-and-keys.md)), and
`src/utils/api/crypto.ts` and `src/utils/api/batch-materializer.ts` are the TypeScript half of
the contracts with the Rust client ([`docs/contracts.md`](../docs/contracts.md)). The rest of this
page is about the cache in `src/utils/cache/`.

## Why there is a cache

`GET /data` (API-033) gives the browser a list of encrypted batches, one for roughly every hour a
device was on. To show any event from a batch, the browser downloads it from R2, unwraps its key,
decrypts, decompresses and decodes it, and checks its hash chain. A week of logs from two devices
is a few hundred batches. Doing that on every page view would be slow on a laptop and painful on a
phone (and it would cost us an R2 read per batch each time).

So the app does the work once. Each decrypted event goes into a SQLite table, each screenshot into
another, and the Logs, Report and Devices pages read from there (WEB-013). The cache keeps 30 days,
the same as the server keeps batches, and never downloads anything older.

## One database, many tabs

The database is opened through SQLite's "SAH pool" VFS, whose OPFS sync access handles are
exclusive: one worker in one tab can have the database open at a time. The app handles several
tabs by electing a **leader** (`src/utils/cache/client.ts`). The tab holding the `cache-leader`
Web Lock starts the worker (`src/utils/cache/worker.ts`) that owns the database. The other tabs
are **followers**, which send their requests to the leader over a `BroadcastChannel` and get the
answers back on the same channel.

When the leader's tab closes, the browser releases the lock, and the next tab in line becomes
leader and replays any requests that were still waiting. A frozen tab is harder, because it keeps
its lock (Android freezes background tabs, for example). If a follower is waiting and the leader
stays silent for long enough, the follower steals the lock (WEB-014). A frozen tab also keeps its
OPFS handles, though, so the new leader can hold the lock and still fail to open the database. The
worker retries for a few seconds, then reports `cache-locked`, which the Logs page shows as
"Another Virtue tab is using saved logs." Much of the retry and timeout code in `client.ts` and
`worker.ts` is there for this handover.

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

A batch that fails to decrypt or decode would fail the same way every time, so it is recorded in
`failed_batch_ids` with its error and skipped until the cache is cleared. A batch that fails
because of the network is left for the next sync (WEB-018). The Decryption stats dialog on the Logs
page lists the recorded failures, and it is the first place to look when someone says their logs
are missing.

## Keeping it right

The cache is shared by everyone who signs in on the browser, so every row carries a `viewer_id`,
and queries read the signed-in viewer's rows. Logging out, or a session expiring,
empties it (WEB-013). The private key reaches the worker as a `CryptoKey` in the
`setSession` message, and switching accounts bumps a session counter, so a sync that started for the
previous person finishes without marking the new person's data as fresh.

The tables are declared in `src/utils/cache/schema.ts`. Every table is created with
`CREATE TABLE IF NOT EXISTS`, which leaves an existing table as it is, so a cache made by an older
release would keep its old columns forever. Opening the database therefore compares what is there
with what is declared, and drops and rebuilds everything on a mismatch (WEB-015). Bump `CACHE_SCHEMA_VERSION` whenever you change a table. Nothing is lost by a
rebuild, since every log can be downloaded and decrypted again.

**Clear cache** (WEB-062) is the bigger hammer. It terminates the worker in every tab, deletes the
OPFS directory outright, and reloads every tab. Deleting the files directly means it still works
when the worker itself is stuck.

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
The tests fake the browser's timing, so if you change how tabs hand over the cache, also try it
by hand with the app open in two tabs, closing one or putting it in the background.
