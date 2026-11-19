# Decision records

Short records of decisions that shaped the system across components, so nobody has to rediscover
why things are the way they are. Platform-specific decisions live in each platform's `DESIGN.md`
instead (for example `client/windows/DESIGN.md`).

Write one when a change settles a question someone is likely to reopen: a trade-off between two
reasonable designs, or something deliberately left out. Keep it to a page.

- Name it `NNNN-short-title.md`, numbered in order. Never reuse a number.
- Sections: **Status** (Accepted, or Superseded by NNNN), **Context**, **Decision**,
  **Consequences**.
- Don't edit an accepted record to change the decision. Write a new one that supersedes it, and
  update the old one's status.

| #                                             | Decision                                               |
| --------------------------------------------- | ------------------------------------------------------ |
| [0001](0001-servers-never-read-content.md)    | Servers never read what a device recorded              |
| [0002](0002-one-sequential-daemon-loop.md)    | The client core is one sequential loop                 |
| [0003](0003-compile-time-client-config.md)    | Client API URL and intervals are fixed at compile time |
| [0004](0004-every-sign-in-is-a-new-device.md) | Every sign-in is a new device                          |
