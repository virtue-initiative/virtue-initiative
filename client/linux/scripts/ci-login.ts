#!/usr/bin/env bun
// Drives `virtue login` non-interactively for CI.
//
// The Linux client reads the password from the controlling terminal in raw
// mode (crossterm), so it cannot be fed over a normal stdin pipe. Bun has no
// built-in pty allocation, so this shells out to the standard `script(1)`
// utility, which always allocates a real pty for its child regardless of its
// own stdio — driving `script`'s piped stdin/stdout gives `virtue login` a
// real controlling tty.
//
// Usage:
//   bun ci-login.ts --bin <path-to-virtue> --email <email> --password <password> --device-name <name>

function parseArgs(argv: string[]): Record<string, string> {
  const args: Record<string, string> = {};
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (!arg.startsWith('--')) continue;
    const key = arg.slice(2);
    const value = argv[i + 1];
    if (value === undefined || value.startsWith('--')) {
      throw new Error(`missing value for --${key}`);
    }
    args[key] = value;
    i++;
  }
  return args;
}

function shQuote(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

const PROMPT_TIMEOUT_MS = 30_000;

async function main() {
  const args = parseArgs(process.argv.slice(2));
  for (const required of ['bin', 'email', 'password', 'device-name']) {
    if (!args[required]) {
      console.error(`ci-login: missing required --${required}`);
      process.exit(2);
    }
  }

  const bin = args.bin;
  const email = args.email;
  const password = args.password;
  const deviceName = args['device-name'];

  const childCmd = [
    shQuote(bin),
    'login',
    '--email',
    shQuote(email),
    '--device-name',
    shQuote(deviceName),
  ].join(' ');

  const proc = Bun.spawn(['script', '-qefc', childCmd, '/dev/null'], {
    stdin: 'pipe',
    stdout: 'pipe',
    stderr: 'inherit',
  });

  const decoder = new TextDecoder();
  let buf = '';
  let sentPassword = false;

  const timeout = setTimeout(() => {
    console.error('ci-login: timed out waiting for the password prompt');
    proc.kill();
  }, PROMPT_TIMEOUT_MS);

  for await (const chunk of proc.stdout as ReadableStream<Uint8Array>) {
    const text = decoder.decode(chunk, { stream: true });
    process.stdout.write(text);
    buf += text;

    if (!sentPassword && buf.includes('Password:')) {
      clearTimeout(timeout);
      // Let raw mode engage before sending.
      await Bun.sleep(200);
      proc.stdin.write(password + '\r');
      await proc.stdin.flush();
      sentPassword = true;
    }
  }

  clearTimeout(timeout);
  const exitCode = await proc.exited;
  if (exitCode !== 0) {
    console.error(`ci-login: virtue login exited with code ${exitCode}`);
  }
  process.exit(exitCode);
}

main().catch((err) => {
  console.error('ci-login: fatal error', err);
  process.exit(1);
});
