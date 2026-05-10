---
title: How our security works
description: A high level overview of how our security works for a non-technical person
pubDate: 2026-05-16
author: The Virtue Initiative team
---

In this article, we will be covering the high-level details of our security -
how it works and what it does. In one sentence, we use end to end encryption to
send screenshots and other data from your computer to your partners, which
means only you and your partner can ever see the screenshots. Our server
cannot (i.e. it doesn't have the key to) access it. The details of how this
works follows.

## Foundations

Before we talk about how our system works, we need to introduce an important
concept called public and private keys. Imagine you and your friend (let's call
her Alice) had an unbreakable safe with a keypad on it, and let's say you have
to enter a code to unlock or lock it. We'll also assume this safe is small
enough to send in the mail repeatedly.

So now imagine Alice wanted to send a secret message to you without Eve (who
handles your mail) reading it. You and Alice meet somewhere and you tell her
what code to use. Now when she wants to send a message to you, she puts it in
the safe, locks it with the code, and mails the safe to you. You open it with
the code and read her message. Success!

But now imagine you and Alice live 1000 miles apart so you can't share the code
at a coffee shop. You could mail Alice the code, but that kinda defeats the
point since Eve could just read your mail. Instead you devise a new system.
You'll update your magical safe to use _two_ code, one can _only_ be used to
lock the safe and the other can _only_ be used to unlock the safe.

Now you send Alice the code to **lock** the safe in the mail. She puts the
message in and locks it but cannot _unlock_ it anymore. Crucially, even though
Eve saw the lock code, _she cannot unlock it either._ Alice mails you the safe
and you unlock it with your **unlock** code and read her message. Success!

This is called public/private key cryptography. Because of some fancy math, we
can create a code that we can use to _only_ "lock" a message which comes with
another code that can _only_ be used to "unlock" a message. If we share the
code that we can use to lock the message to our friends (since it is shared, we
call it the "public key"), then they can lock messages and send them to us
without anyone being able to read them, **but** since we have the unlock code
(called a "private key" since we keep it private), we can unlock them and read
them.

This is essential for our system since the goal is that our own system (i.e.
the "mailman" for your screenshots) **cannot** see them even though we store
them on our servers.

## How our security works

When you sign up for an account, your computer uses your password to create a
pair of keys, one for locking screenshots and one for unlocking screenshots. It
sends the **locking** (i.e. the public) key to our servers and it keeps the
**unlocking** (the private) key to itself.

When someone adds you as a partner, our server gives them the locking key and
they begin using it to lock their screenshots before sending them to the
server. Since we don't have the unlocking key (it stays on your computer), our
server just see random data and cannot see the screenshots, but when you
download the locked screenshots from our server, you have the right unlocking
key and can see them just fine.

## Handling passwords

You might have noticed that we use the password to generate the public/private
keys, which might make you question why the server couldn't just use the
password you sent them to login to get your private (unlocking) key.

The server could **if** we actually sent your password to the server, but we
**don't** and here's how. Imagine you have a secret recipe for an amazing
chocolate cake. Let's say you got it from your great great grandma. Now let's
say you find a long lost distant relative that also claims to have the secret
recipe from your great grandma. You're skeptical but neither of you are willing
to compare recipes since that would be giving away the secret. What's the
solution? Have your long lost relative bake a cake with the recipe, if it
matches your cake, you know you both have the same recipe even though neither
of you ever saw each other's recipes.

We use a similar concept for handling passwords. Imagine your password is a
recipe for a cake. When you sign up, instead of sending the password itself, we
send the server the "cake" you made with your password recipe. The server then
remembers this "cake". Now when you log into your account, you make another
"cake" with the same recipe and send it to the server, the server checks if
both "cakes" match and then lets you log in. Importantly, the server only ever
sees the **"cake"** not the password itself, so we can still use the password
for your public and private keys.

## The end

This is a very high-level overview of how our security works. There's a lot
more details and complexity when you actually look at it (like handling
multiple partners without making multiple copies of the screenshots) and other
things that improve security like not actually creating the public/private
keys from the password, but instead generating them randomly and putting them
in a "personal safe" that can only be unlocked with your password and sending
that safe to the server.

If you're interested in the raw details, you can take a look at our
[cryptography and security](/help/developer/security) page which aims to
completely explain every part of it, but be warned, it assumes a decent
knowledge of cryptography.

I hope these explainations made sense, if you have any questions feel free to
reach out on [Discord](https://discord.gg/4kNsbRuzQD) or by email at
help@virtueinitiative.org.
