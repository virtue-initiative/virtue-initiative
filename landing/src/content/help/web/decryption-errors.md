---
sidebar_position: 3
---

# Decryption errors

Every screenshot batch a device uploads is encrypted before it leaves the
device, and only decrypted in your browser when you view your logs (see
[Security and encryption](/help/developer/security)). Occasionally a batch
permanently fails to decrypt. This page explains why, and what — if
anything — you need to do about it.

Batch decryption failures are visible in the **Decryption stats** dialog on
the Logs page (the info icon next to the sync status line), grouped by the
underlying error message.

## Common causes

There are two real causes of a permanent decryption failure:

### Password reset

Resetting your password always generates a new private key — there's no way
to recover the old one. Any batch encrypted before the reset can no longer
be opened afterward. This is expected and not a sign of data loss; it's a
side effect of how the reset flow re-establishes your key from scratch.

### Client/server version mismatch

A client running a very old or very new version may produce a batch format
the web app doesn't understand (or vice versa). Updating the client to the
latest version prevents this for future batches; already-uploaded batches in
the old format may still fail.

## What to do

A handful of permanently failed batches is normal after a password reset and
not a cause for concern. If failures aren't explained by a recent password
reset or a known version mismatch, [report a bug](/help) with the error
message shown in the dialog so we can investigate.
