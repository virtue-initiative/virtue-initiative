# Cryptography and security

Last Updated: 2025-03-15

The following is a document that lays out our security model and the
cryptography that we use.

## High level overview

The main goal of this project is to send screenshots and logs from client devices to partners.

Our sensitive data:
- User email and password
- PwdKey: Password derived symmetric key (`PBKDF2(pwd, userid)`)
- E2EE encryption key
- User private/public keypair
- Low Quality Screenshots
- Logs

Data storage:

| Item            | Place   | Format                        | Access           |
|-----------------|---------|-------------------------------|------------------|
| Password        | Server  | `SHA256(Argon2(pwd+email))`   | None             |
| PwdKey          | Browser | Plaintext                     | Browser          |
| Email           | Server  | Plaintext                     | Partners         |
| E2EE key        | Server  | `AES-GCM(pwdkey)`             | User             |
| E2EE key        | Server  | `RSA(partner pubkey)`         | Partner          |
| E2EE key        | Device  | Plaintext                     | Device           |
| Private Key     | Server  | `AES-GCM(pwdkey)`             | User             |
| Private Key     | Browser | Plaintext                     | Browser          |
| Public Key      | Server  | Plaintext                     | Anyone           |
| Screenshot List | Server  | Plaintext                     | Partners         |
| Screenshots     | Server  | `AES-GCM(E2EE key)`           | Anyone with UUID |
| Screenshots     | Device  | Plaintext                     | Device           |
| Logs            | Server  | Plaintext                     | Partners         |
| Logs            | Device  | Plaintext                     | Device           |

### Passwords

Passwords are used for authentication and encryption.

When a user signs up. Their password is hashed with Argon2 (using their email
as a deterministic salt) and sent to the server. The server hashes it with
SHA256 and stores it in the DB.

It is also combined with the user's UUID and used to derive an encryption key
(PwdKey) which is stored in the user's browser.

When a user or device logs in, the browser rehashes the password with Argon2
and sends it to the server, which hashes it and compares it with the stored
hash.

