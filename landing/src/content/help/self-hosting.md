---
title: 'Connecting the apps to your own server'
description: 'Sign in to a server you run yourself from the Virtue apps.'
navTitle: 'Your own server'
topic: account
order: 7
---

# Connecting the apps to your own server

The Virtue apps sign in to the Virtue Initiative server by default. If you run
your own copy of the server, you can sign in to it from the same apps,
without building your own.

## What your server needs

- The API, served over HTTPS. The apps refuse plain `http://` except for
  `localhost`, `127.0.0.1` and `[::1]`, which are only useful for testing on
  the same machine.
- `APP_URL` set to the address of your web app. The apps read it from the
  API, and use it for the Partners link once you are signed in.
- The hash server and web app the API points to. The apps get the hash
  server's address from the API, so there is nothing else to enter.

See the [API README](https://github.com/virtue-initiative/virtue-initiative/blob/main/api/README.md)
for how to deploy the API.

## Signing in

On the sign-in screen, choose **Self-hosted** under **Server** and enter your
server's address, such as `virtue.example.org`. You can enter either the API's
own address or the address of a web app that serves the API under `/api`. The
app adds `https://` when you leave it out, and checks that the address answers
as a Virtue server before it sends your email and password.

On Linux, add `--server`:

```sh
virtue login --server virtue.example.org
```

Create your account on your own server's website first. The sign-up link in
the apps only goes to the Virtue Initiative server, so the apps hide it while
**Self-hosted** is chosen.

## Changing servers

The server can only be changed while signed out. To move a device to another
server, sign out and sign in again. Signing out deactivates the device, and
your partners may be alerted, the same as any other sign-out.

To see which server a device is using, open Status Details. The API URL row
under Advanced shows it.
