---
title: 'How your data is kept private'
description: 'What is encrypted, who can read it, and what the Virtue servers can see.'
topic: account
order: 4
---

Your screenshots are encrypted on your device before they are uploaded, and only you and your
partners hold the keys to read them. The Virtue team cannot see them.

## Screenshots

Before a screenshot leaves your device, Virtue shrinks it, blurs it, and blacks out any text it
finds. Screenshots are then encrypted in batches. Each batch gets its own key, and that key is
locked separately for you and for each of your partners. The servers store only the encrypted
batches and the locked keys, so only you and your partners can open them.

## Your password

Your password never leaves your browser or device. Virtue turns it into two keys: one to log
you in, and one that unlocks your account's private key. That private key is what lets you read
your screenshots, and it is stored only in its locked form.

This is why [resetting a forgotten password](/help/reset-password) makes older activity
unreadable. Without the old password, nobody can unlock the old key.

## What the servers can read

Some things are not encrypted, because Virtue needs them to work:

- your email address and name, and who your partners are;
- your devices and when each one last checked in;
- the other entries in your log feed, such as sign-ins and alerts, so Virtue can email alerts
  and summaries to your partners.

## Locked passwords

A [locked password](/help/web/locked-passwords) is encrypted so only you can read it. Your
partners are told when you reveal one, but never what it says.

For the full technical details, see [security details](/help/security).
