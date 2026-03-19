# Cryptography and security

Last Updated: 2026-03-18

The following is a document that lays out our security model and the
cryptography that we use.

## High level overview

The main goal of this project is to send screenshots and logs from client devices to partners.

Our sensitive data:
- User email and password
- User private/public keypair
- Low Quality Screenshot batches
- Logs

Data storage:

| Item            | Place   | Format                         | Access           |
|-----------------|---------|--------------------------------|------------------|
| Password        | Server  | `SHA256(Argon2(pwd))`          | None             |
| Email           | Server  | Plaintext                      | Partners         |
| Private Key     | Server  | `AES-GCM(Argon2(pwd))`         | User             |
| Private Key     | Browser | Plaintext                      | Browser          |
| Private Key     | Device  | Plaintext                      | Device           |
| Public Key      | Server  | Plaintext                      | Anyone           |
| Batch List      | Server  | Plaintext                      | Partners         |
| Batches         | Server  | `AES-GCM(Public Key)`          | Anyone with UUID |
| Latest Batch    | Device  | Plaintext                      | Device           |
| Logs            | Server  | Plaintext                      | Partners         |
| Logs            | Device  | Plaintext                      | Device           |

## Details

### Passwords

Passwords are used for authentication and encryption.

When a user signs up, they make a request to get the current hash parameters
and then generate a random salt. They then hash the plaintext password using
Argon2 with those parameters and the salt. They upload
`SHA256(Argon2(password)), salt` to the server.

They also generate a private key and encrypt the private key with
`AES-GCM(Argon2(password))` and upload it to the server. The plaintext private
key is stored in the browser (as a non-exportable key).

On login, the browser/device requests the hash parameters and salt from the
server and then produces `Argon2(password)`. Next it logs in using
`SHA256(Argon2(password))`. Next, it downloads the encrypted private key and
decrypts it with `AES-GCM(Argon2(password))`. Finally, it stores the private
key locally (if it is the browser, it stores it as a non-exportable key).

### Emails

Emails are uploaded to the server on signup/login and can be viewed by
partners. They are stored in plaintext.

### Private Key

This key is created on signup and encrypted with `Argon2(password)` before
being sent to the server. It is stored in the browser for decrypting screenshot
batch decryption keys.

On login, the browser downloads the encrypted key from the server and decrypts
it with the user's password key and it is stored in the browser.

This key is recreated on a password reset.

See the **Passwords** section for more information.

### Public Key

This key is generated alongside the private key on signup. It is uploaded in
plaintext to the server. The devices download a list of all the current
partners' public keys and encrypt the batch encryption key with all of them.

### Batch list

This is the list of batches stored in the database. The user and their partners
can access it.

Each item consists of the following data about the batch which is stored in
plaintext.

```
owner
start time
end time
download url
verification hash
encryption keys
  encrypted encryption key for user
  encrypted encryption key for partner 1
  encrypted encryption key for partner 2
  ...
```

Batches and the batch items are deleted after 30 days.

### Batches

Encrypted batches are stored in publically accessible object storage. Their
URLs look like `/[user]/[batch id]`.

They are encrypted with a random key which is stored (encrypted) in the
database (see **Batch list**).

The in-progress batch is stored in plaintext on the device, but is encrypted
before being sent to the server.

## Known tradeoffs

- We ultimately encrypt everything with the user's password. Using the user's
  private key would be better, but implementing a good UX for key management is
  non-trivial.
- We provide a browser app, so we could still send malicious JS code that
  exfiltrates the data after it is decrypted. We might try to think about a
  solution in the future, but that is a hard problem to solve.

## Possible weaknesses or improvements

- Emails might currently be able to be guessed, either in the login flow or in
  the invite flow.

