#!/usr/bin/env bun
// Drives `virtue login --password` non-interactively for CI.
//
// `virtue login` now defaults to the pairing-code flow, which needs a human at
// a logged-in web session to approve the code. CI pins the password path with
// `--password` instead.
//
// The Linux client reads the password from the controlling terminal in raw
// mode (crossterm), so it cannot be fed over a normal stdin pipe. Bun has no
// built-in pty allocation, so this shells out to the standard `script(1)`
// utility, which always allocates a real pty for its child regardless of its
// own stdio — driving `script`'s piped stdin/stdout gives `virtue login` a
// real controlling tty.
//
// Exports `ciLogin()` for use from integration-test.ts; also runnable
// standalone:
//   bun ci-login.ts --bin <path-to-virtue> --email <email> --password <password> --device-name <name>

const PROMPT_TIMEOUT_MS = 30_000;

function shQuote(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

export interface CiLoginArgs {
  bin: string;
  email: string;
  password: string;
  deviceName: string;
  env?: Record<string, string>;
}

/** Runs `<bin> login --password`, feeding the password once the pty prompt appears. Throws on failure. */
export async function ciLogin(args: CiLoginArgs): Promise<void> {
  const childCmd = [
    shQuote(args.bin),
    'login',
    '--password',
    '--email',
    shQuote(args.email),
    '--device-name',
    shQuote(args.deviceName),
  ].join(' ');

  const proc = Bun.spawn(['script', '-qefc', childCmd, '/dev/null'], {
    env: args.env ?? (process.env as Record<string, string>),
    stdin: 'pipe',
    stdout: 'pipe',
    stderr: 'inherit',
  });

  const decoder = new TextDecoder();
  let buf = '';
  let sentPassword = false;
  let timedOut = false;

  const timeout = setTimeout(() => {
    timedOut = true;
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
      proc.stdin.write(args.password + '\r');
      await proc.stdin.flush();
      sentPassword = true;
    }
  }

  clearTimeout(timeout);
  const exitCode = await proc.exited;
  if (timedOut) throw new Error('ci-login: timed out waiting for the password prompt');
  if (exitCode !== 0) throw new Error(`ci-login: virtue login exited with code ${exitCode}`);
}

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

if (import.meta.main) {
  const args = parseArgs(process.argv.slice(2));
  for (const required of ['bin', 'email', 'password', 'device-name']) {
    if (!args[required]) {
      console.error(`ci-login: missing required --${required}`);
      process.exit(2);
    }
  }

  ciLogin({
    bin: args.bin,
    email: args.email,
    password: args.password,
    deviceName: args['device-name'],
  }).catch((err) => {
    console.error(err instanceof Error ? err.message : err);
    process.exit(1);
  });
}
