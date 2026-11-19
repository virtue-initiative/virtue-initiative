# 0001: Servers never read what a device recorded

**Status:** Accepted

## Context

Screenshots of someone's screen are about as private as data gets. An accountability tool needs
partners to see them, but nobody else should, including whoever runs the servers, an attacker
who gets into them, or anyone who can compel the operator to hand data over.

## Decision

Events are encrypted on the device and decrypted only in a reader's browser (BATCH-001). Each
batch has its own key, wrapped for each reader's public key. Private keys are stored only
encrypted with a key derived from the user's password, which the server never receives.

What the server needs to work without reading content, it gets as separate metadata: the
device sends counts of high and medium risk events with each batch (API-038), and that is what
digest and alert emails are built from.

## Consequences

- A forgotten password can't recover old logs. Resetting it creates a new key pair.
- Adding a partner only gives them batches uploaded after the device learns their key.
- Any feature that needs content on the server (search, server-side image review, previews in
  email) is out unless it can be built from metadata the device chooses to send.
- The crypto and batch format are implemented twice, in Rust and TypeScript, and must match
  bit for bit. Shared test vectors (BATCH-008) keep them in step.
