# Developer overrides

The API URL, capture interval, and batch window are compile-time settings. They
are baked into the client when it is built, so there is no config file or
in-app override. To change them for development, set the variables below and
rebuild the client.

## Settings

| Variable                                  | Default                                                          | Minimum |
| ----------------------------------------- | ---------------------------------------------------------------- | ------- |
| `VIRTUE_DEFAULT_API_URL`                  | The production API on `main`, the staging API on other branches. | None    |
| `VIRTUE_DEFAULT_CAPTURE_INTERVAL_SECONDS` | `300`                                                            | `15`    |
| `VIRTUE_DEFAULT_BATCH_WINDOW_SECONDS`     | `3600`                                                           | `1`     |

A value that is not a number, or is below its minimum, fails the build.

## Where to set them

Set them in the repo-root `.env` file. Copy `.env.example` to `.env` and
uncomment the lines you need. `.env` is gitignored, so each worktree can have
its own values.

```sh
VIRTUE_DEFAULT_API_URL=http://localhost:8787
VIRTUE_DEFAULT_CAPTURE_INTERVAL_SECONDS=15
VIRTUE_DEFAULT_BATCH_WINDOW_SECONDS=60
```

The build looks for each variable in this order and uses the first one it finds:

1. A real environment variable, such as one set in CI or in your shell.
2. The repo-root `.env`.
3. `~/.config/virtue-dev.env`, for values shared by every worktree on your
   machine. Set `VIRTUE_DEV_ENV` to use a different path.

## Notes

- Rebuild the client after changing a value. The new value takes effect on the
  next build, not on a running app.
- Android emulators should use `http://10.0.2.2:8787`, not `http://localhost:8787`.
- On Linux, `virtue status` prints the values the build used.
- When switching between prod and local APIs, rebuild, then log out and log back
  in.
