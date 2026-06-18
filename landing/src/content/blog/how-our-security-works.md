---
title: How our security works
description: A high-level overview of how our security works for a non-technical person
pubDate: 2026-05-16
author: The Virtue Initiative team
---

In this article, we will be covering the high-level details of our security -
how it works and what it does. In one sentence, we use end-to-end encryption to
send screenshots and other data from your computer to your partners, which
means only you and your partner can ever see the screenshots. Our server
cannot (i.e. it doesn't have the key to) access it. The details of how this
works follow.

## Foundations

Before we talk about how our system works, we need to introduce an important
concept called public and private keys.

Imagine you and your friend wanted to send a secret message through the mail
without whoever handles your mail being able to read it. One way to do this
would be to meet somewhere and make up a secret code, but let's say you can't
meet due to distance.

Let's say you invented a special paint that only you knew the chemical required
to dissolve it and sent it to your friend. Your friend writes their message and
then puts a coating of your special paint over it. Now no one can read the
message unless they know how to dissolve the paint, which means it's safe from
the mailman, but you can still read it since you know how to dissolve the
paint.

Public and private keys are a similar system where (using some math) one person
can create something like a digital "coating" (called the public key) which can
only be dissolved with the "special chemical" (called the private key). (In
reality, these are just huge special numbers that we do some fancy math with.)

So in the digital world, when you want someone to send a secret message to you,
all you need to do is send them the _public key_ (or "coating"). They take the
message they want to send, "put the coating over it", and send it to you. Now
there's no need to worry about anyone else reading it since you're the only one
who can "dissolve the coating" since you have the _private key_.

This is essential for our system since the goal is that our own system (i.e.
the "mailman" for your screenshots) **cannot** see them even though we store
them on our servers.

## How our security works

When you sign up for an account, your computer uses your password to create a
pair of keys, one for "coating" (**encrypting**) screenshots and another for
"dissolving the coating on" (**decrypting**) screenshots. It sends the
"coating" (i.e. the public) key to our servers, and it keeps the "dissolving"
(the private) key to itself.

When someone adds you as a partner, our server gives them the "coating" key and
they begin using it to "coat" their screenshots before sending them to the
server. Since we don't have the "dissolving" key (it stays on your computer),
our server just sees random data and cannot see the screenshots, but when you
download the "coated" screenshots from our server, you have the right
"dissolving" key and can see them just fine.

## Handling passwords

You might have noticed that we use the password to generate the public/private
keys, which might make you question why the server couldn't just use the
password you sent them to login to get your private ("dissolving") key.

The server could **if** we actually sent your password to the server, but we
**don't** and here's how. Imagine you have a secret recipe for an amazing
chocolate cake. Let's say you got it from your great-great-grandma. Now, let's
say you find a long-lost distant relative who also claims to have the secret
recipe from your great-grandma. You're skeptical, but neither of you is willing
to compare recipes since that would be giving away the secret. What's the
solution? Have your long-lost relative bake a cake with the recipe. If it
matches your cake, you know you both have the same recipe, even though neither
of you ever saw each other's recipes.

We use a similar concept for handling passwords. Imagine your password is a
recipe for a cake. When you sign up, instead of sending the password itself, we
send the server the "cake" you made with your password recipe. The server then
remembers this "cake". Now, when you log into your account, you make another
"cake" with the same recipe and send it to the server. The server checks if
both "cakes" match and then lets you log in. Importantly, the server only ever
sees the **"cake"**, not the password itself, so we can still use the password
for your public and private keys.

## The end

This is a very high-level overview of how our security works. There's a lot
more details and complexity when you actually look at it (like handling
multiple partners without making multiple copies of the screenshots) and other
smaller things that improve security.

If you're interested in the raw details, you can take a look at our
[cryptography and security](/help/developer/security) page which aims to
completely explain every part of it, but be warned, it assumes a decent
knowledge of cryptography.

I hope these explainations made sense, if you have any questions feel free to
reach out on [Discord](https://discord.gg/4kNsbRuzQD) or by email at
help@virtueinitiative.org.
