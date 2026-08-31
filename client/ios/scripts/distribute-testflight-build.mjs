/**
 * Distributes an already-uploaded TestFlight build to a named beta group.
 *
 * `build-and-upload-testflight.sh` ends at `xcrun altool --upload-app`, which
 * hands the .ipa to App Store Connect and returns immediately. That alone puts
 * the build nowhere: it processes for several minutes, then sits in TestFlight
 * attached to no group. Everything after the upload — waiting for processing,
 * answering export compliance, writing "What to Test", attaching the build to a
 * group, and submitting it for beta review — is App Store Connect state that
 * only the App Store Connect API can set. altool cannot do any of it.
 *
 * Run with plain `node` and no dependencies: it needs ES256 JWT signing, and
 * `crypto.sign(..., { dsaEncoding: 'ieee-p1363' })` produces exactly the raw
 * r||s signature JWT wants, so the one hard part is a single stdlib call.
 *
 * Required env:
 *   IOS_ASC_KEY_ID, IOS_ASC_ISSUER_ID, IOS_ASC_API_KEY_PATH  (.p8 private key)
 *   IOS_BETA_GROUP_NAME     TestFlight group to distribute to, matched by name
 *   IOS_BUILD_VERSION       CFBundleVersion of the build just uploaded
 *   IOS_MARKETING_VERSION   CFBundleShortVersionString of that build
 * Optional env:
 *   IOS_BUNDLE_ID                       default org.virtueinitiative.virtueios
 *   IOS_WHATS_NEW                       "What to Test" text
 *   IOS_EXCLUSIVE_BETA_GROUP            "true" to remove the build from every
 *                                       other beta group (see below)
 *   IOS_PROCESSING_TIMEOUT_SECONDS      default 2700 (45 minutes)
 */

import { createPrivateKey, sign as cryptoSign } from 'node:crypto';
import { readFile } from 'node:fs/promises';

const ASC_BASE_URL = 'https://api.appstoreconnect.apple.com';
const DEFAULT_BUNDLE_ID = 'org.virtueinitiative.virtueios';
const DEFAULT_PROCESSING_TIMEOUT_SECONDS = 45 * 60;
const PROCESSING_POLL_INTERVAL_MS = 30 * 1000;
// The JWT is minted once per run and ASC caps token lifetime at 20 minutes,
// so a run that outlives one token has to re-mint. Refresh well before expiry.
const TOKEN_LIFETIME_SECONDS = 19 * 60;
const TOKEN_REFRESH_MARGIN_SECONDS = 60;

function requireEnv(name) {
  const value = process.env[name];
  if (!value || !value.trim()) {
    throw new Error(`${name} is required`);
  }
  return value.trim();
}

function base64url(input) {
  return Buffer.from(input).toString('base64url');
}

class AppStoreConnectClient {
  #privateKey;
  #keyId;
  #issuerId;
  #token = null;
  #tokenExpiresAt = 0;

  constructor({ privateKeyPem, keyId, issuerId }) {
    this.#privateKey = createPrivateKey(privateKeyPem);
    this.#keyId = keyId;
    this.#issuerId = issuerId;
  }

  #mintToken() {
    const nowSeconds = Math.floor(Date.now() / 1000);
    const header = { alg: 'ES256', kid: this.#keyId, typ: 'JWT' };
    const payload = {
      iss: this.#issuerId,
      iat: nowSeconds,
      exp: nowSeconds + TOKEN_LIFETIME_SECONDS,
      aud: 'appstoreconnect-v1',
    };

    const signingInput = `${base64url(JSON.stringify(header))}.${base64url(JSON.stringify(payload))}`;
    // ES256 requires the raw 64-byte r||s form. Node's default for EC keys is
    // DER, which App Store Connect rejects as an invalid signature.
    const signature = cryptoSign('sha256', Buffer.from(signingInput), {
      key: this.#privateKey,
      dsaEncoding: 'ieee-p1363',
    });

    this.#token = `${signingInput}.${signature.toString('base64url')}`;
    this.#tokenExpiresAt = nowSeconds + TOKEN_LIFETIME_SECONDS;
  }

  #authorization() {
    const nowSeconds = Math.floor(Date.now() / 1000);
    if (!this.#token || nowSeconds >= this.#tokenExpiresAt - TOKEN_REFRESH_MARGIN_SECONDS) {
      this.#mintToken();
    }
    return `Bearer ${this.#token}`;
  }

  /**
   * @returns {Promise<{status: number, body: any}>} Never throws for a
   * documented HTTP status — callers decide which ones are tolerable, since a
   * 409 means "already done" for some of these endpoints and "genuinely broken"
   * for others.
   */
  async request(method, path, body, { allowStatuses = [] } = {}) {
    const url = path.startsWith('http') ? path : `${ASC_BASE_URL}${path}`;
    let lastError = null;

    for (let attempt = 1; attempt <= 5; attempt += 1) {
      let response;
      try {
        response = await fetch(url, {
          method,
          headers: {
            Authorization: this.#authorization(),
            'Content-Type': 'application/json',
            Accept: 'application/json',
          },
          body: body === undefined ? undefined : JSON.stringify(body),
        });
      } catch (error) {
        // Network-level failure (DNS, TLS, socket reset) — always worth a retry.
        lastError = error;
        await sleep(attempt * 2000);
        continue;
      }

      const text = await response.text();
      const parsed = text ? safeJsonParse(text) : null;

      if (response.ok || allowStatuses.includes(response.status)) {
        return { status: response.status, body: parsed };
      }

      // 429 is rate limiting and 5xx is App Store Connect being briefly
      // unavailable; both are transient. Every other status is a real answer.
      if (response.status !== 429 && response.status < 500) {
        throw new Error(
          `${method} ${url} failed: ${response.status} ${formatAscErrors(parsed) || text}`,
        );
      }

      lastError = new Error(
        `${method} ${url} failed: ${response.status} ${formatAscErrors(parsed) || text}`,
      );
      await sleep(attempt * 5000);
    }

    throw lastError ?? new Error(`${method} ${url} failed after retries`);
  }
}

function safeJsonParse(text) {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

function formatAscErrors(body) {
  if (!body?.errors?.length) return '';
  return body.errors
    .map((error) => {
      const parts = [error.code, error.title, error.detail].filter(Boolean);
      return parts.join(' — ');
    })
    .join(' | ');
}

function sleep(ms) {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

async function resolveAppId(client, bundleId) {
  const { body } = await client.request(
    'GET',
    `/v1/apps?filter[bundleId]=${encodeURIComponent(bundleId)}&limit=1`,
  );
  const app = body?.data?.[0];
  if (!app) {
    throw new Error(`No app found in App Store Connect with bundle id ${bundleId}`);
  }
  console.log(`App ${bundleId} -> id ${app.id}`);
  return app.id;
}

async function waitForProcessedBuild(client, { appId, buildVersion, marketingVersion, timeoutMs }) {
  const deadline = Date.now() + timeoutMs;
  const query =
    `/v1/builds?filter[app]=${encodeURIComponent(appId)}` +
    `&filter[version]=${encodeURIComponent(buildVersion)}` +
    `&filter[preReleaseVersion.version]=${encodeURIComponent(marketingVersion)}&limit=1`;

  while (true) {
    const { body } = await client.request('GET', query);
    const build = body?.data?.[0];
    const state = build?.attributes?.processingState;

    if (build && state === 'VALID') {
      console.log(`Build ${marketingVersion} (${buildVersion}) -> id ${build.id}, processed.`);
      return build;
    }

    if (state === 'FAILED' || state === 'INVALID') {
      throw new Error(
        `Build ${marketingVersion} (${buildVersion}) finished processing as ${state}; it cannot be distributed. Check App Store Connect for the rejection reason.`,
      );
    }

    if (Date.now() >= deadline) {
      throw new Error(
        `Timed out after ${Math.round(timeoutMs / 60000)} minutes waiting for build ${marketingVersion} (${buildVersion}) to finish processing. Last state: ${state ?? 'not yet visible'}.`,
      );
    }

    console.log(
      `Build ${marketingVersion} (${buildVersion}) is ${state ?? 'not visible yet'}; checking again in ${PROCESSING_POLL_INTERVAL_MS / 1000}s.`,
    );
    await sleep(PROCESSING_POLL_INTERVAL_MS);
  }
}

/**
 * Fails fast if the build has no export compliance answer.
 *
 * `ITSAppUsesNonExemptEncryption` in Info.plist answers this at build time, so
 * App Store Connect reports it on the build and there is nothing to set here.
 * Without it the build sits in TestFlight as "Missing Compliance" and reaches
 * nobody, which is worth stopping the release for rather than distributing into
 * a void.
 *
 * "Non-exempt" in that key means *exempt from export documentation
 * requirements*, not "isn't encryption". `client/core` implements AES-256-GCM,
 * HPKE, Argon2id and HKDF rather than calling Apple's CryptoKit, which by
 * Apple's classification is "industry standard algorithms not provided within
 * the Apple operating system" — that tier needs no CCATS, only a French
 * declaration where applicable, which is why App Store Connect reports that no
 * documents are required.
 */
function assertExportCompliance(build) {
  const answer = build.attributes?.usesNonExemptEncryption;
  if (typeof answer !== 'boolean') {
    throw new Error(
      'Build has no export compliance answer, so it would sit in TestFlight as "Missing ' +
        'Compliance" and reach no testers. Is ITSAppUsesNonExemptEncryption still declared in ' +
        'client/ios/app/Info.plist?',
    );
  }
  console.log(`Export compliance declared in Info.plist (usesNonExemptEncryption=${answer}).`);
}

/**
 * External beta review is refused outright when the build has no "What to Test"
 * text, so this has to run before the review submission, not after.
 */
async function setWhatToTest(client, { build, whatsNew }) {
  const locale = 'en-US';
  const { body } = await client.request(
    'GET',
    `/v1/betaBuildLocalizations?filter[build]=${encodeURIComponent(build.id)}&limit=200`,
  );
  const existing = (body?.data ?? []).find((entry) => entry.attributes?.locale === locale);

  if (existing) {
    await client.request('PATCH', `/v1/betaBuildLocalizations/${existing.id}`, {
      data: {
        type: 'betaBuildLocalizations',
        id: existing.id,
        attributes: { whatsNew },
      },
    });
  } else {
    await client.request('POST', '/v1/betaBuildLocalizations', {
      data: {
        type: 'betaBuildLocalizations',
        attributes: { locale, whatsNew },
        relationships: { build: { data: { type: 'builds', id: build.id } } },
      },
    });
  }

  console.log(`Set "What to Test" (${locale}).`);
}

/**
 * Is this build a member of that group?
 *
 * Asked from the builds side on purpose. The obvious direction,
 * `GET /v1/builds/{id}/betaGroups`, is rejected by App Store Connect with
 * "The relationship 'betaGroups' does not allow 'GET_RELATED'. Allowed
 * operations are: CREATE, DELETE" — that relationship is write-only. Filtering
 * builds by `filter[betaGroups]` reads the same fact in a supported direction.
 */
async function buildIsInGroup(client, { appId, buildVersion, marketingVersion, groupId, buildId }) {
  const { body } = await client.request(
    'GET',
    `/v1/builds?filter[app]=${encodeURIComponent(appId)}` +
      `&filter[version]=${encodeURIComponent(buildVersion)}` +
      `&filter[preReleaseVersion.version]=${encodeURIComponent(marketingVersion)}` +
      `&filter[betaGroups]=${encodeURIComponent(groupId)}&limit=1`,
  );
  return body?.data?.[0]?.id === buildId;
}

/**
 * Keeps the build out of every group except the target one.
 *
 * `client/core/build.rs` bakes the API base URL in at compile time from the
 * release channel: a `main` build talks to https://api.virtueinitiative.org and
 * a `staging` build to https://staging.app.virtueinitiative.org/api. So a
 * stable build reaching the staging testers' group would silently move them
 * onto production — real accounts, real data. Group membership is the only
 * thing standing between the two.
 *
 * Internal groups with `hasAccessToAllBuilds` receive every build automatically
 * as soon as it is distributable, which is why this has to actively remove the
 * build rather than just decline to add it. That auto-add is asynchronous, so a
 * single pass can race it; hence the re-check loop.
 *
 * External groups never auto-receive builds, so if nothing is reported here
 * there was nothing to prevent.
 */
async function enforceExclusiveGroup(client, context) {
  const { build, group, groups, appId, buildVersion, marketingVersion } = context;
  const MAX_PASSES = 3;
  const others = groups.filter((candidate) => candidate.id !== group.id);

  if (others.length === 0) {
    console.log(`Verified: "${group.name}" is the app's only beta group.`);
    return;
  }

  let strays = [];
  for (let pass = 1; pass <= MAX_PASSES; pass += 1) {
    strays = [];
    for (const other of others) {
      const present = await buildIsInGroup(client, {
        appId,
        buildVersion,
        marketingVersion,
        groupId: other.id,
        buildId: build.id,
      });
      if (present) strays.push(other);
    }

    if (strays.length === 0) {
      console.log(`Verified: build is in "${group.name}" and no other beta group.`);
      return;
    }

    for (const stray of strays) {
      const name = stray.attributes?.name ?? stray.id;
      await client.request(
        'DELETE',
        `/v1/betaGroups/${stray.id}/relationships/builds`,
        { data: [{ type: 'builds', id: build.id }] },
        { allowStatuses: [404, 409] },
      );
      console.log(`Removed build from unintended beta group "${name}".`);

      if (stray.attributes?.hasAccessToAllBuilds) {
        console.log(
          `  NOTE: "${name}" has "automatically distribute new builds" enabled, so it will ` +
            'keep picking up every build until that is turned off in App Store Connect. ' +
            'Removing it here is a cleanup, not a guarantee — a tester could briefly have ' +
            'seen the build.',
        );
      }
    }

    // The auto-add can land between the check and the delete, so confirm on the
    // next pass rather than trusting one round of deletes.
    await sleep(5000);
  }

  const names = strays.map((entry) => `"${entry.attributes?.name ?? entry.id}"`).join(', ');
  throw new Error(
    `Build is still attached to ${names} after ${MAX_PASSES} removal passes. Those testers ` +
      'would be pointed at the wrong API. Turn off "automatically distribute new builds" ' +
      'for those groups in App Store Connect.',
  );
}

async function resolveBetaGroup(client, { appId, groupName }) {
  // Matched client-side rather than with filter[name]: an exact-match filter
  // that silently returns nothing would be indistinguishable from a typo'd
  // group name, and listing lets the error name the groups that do exist.
  const { body } = await client.request(
    'GET',
    `/v1/betaGroups?filter[app]=${encodeURIComponent(appId)}&limit=200`,
  );
  const groups = body?.data ?? [];
  const match = groups.find((group) => group.attributes?.name === groupName);

  if (!match) {
    const available = groups.map((group) => `"${group.attributes?.name}"`).join(', ') || '(none)';
    throw new Error(
      `No TestFlight beta group named "${groupName}" on this app. Existing groups: ${available}`,
    );
  }

  const isInternal = match.attributes?.isInternalGroup === true;
  console.log(
    `Beta group "${groupName}" -> id ${match.id} (${isInternal ? 'internal' : 'external'}).`,
  );
  // `groups` comes back too: enforceExclusiveGroup must check every other group
  // on the app, and this response already holds them.
  return { group: { id: match.id, name: groupName, isInternal }, groups };
}

async function addBuildToGroup(client, { group, build }) {
  // 409 here is the "build is already in this group" case, which a re-run of a
  // partially-failed job hits routinely. Membership is verified below either
  // way, so tolerating it costs nothing and re-runs stay idempotent.
  const { status, body } = await client.request(
    'POST',
    `/v1/betaGroups/${group.id}/relationships/builds`,
    { data: [{ type: 'builds', id: build.id }] },
    { allowStatuses: [409] },
  );

  if (status === 409) {
    console.log(`Group add returned 409 (${formatAscErrors(body) || 'no detail'}); verifying.`);
  }
}

async function verifyGroupMembership(client, context) {
  const { appId, group, build, buildVersion, marketingVersion } = context;
  const present = await buildIsInGroup(client, {
    appId,
    buildVersion,
    marketingVersion,
    groupId: group.id,
    buildId: build.id,
  });

  if (!present) {
    throw new Error(
      `Build ${buildVersion} is not a member of beta group ${group.id} after the add call. It has not been distributed.`,
    );
  }

  console.log('Verified: build is attached to the beta group.');
}

/**
 * External groups can only receive a build once it has cleared Beta App Review.
 * Internal groups never need it, so this is skipped for them.
 */
async function submitForBetaReview(client, { build }) {
  // No "is it already submitted" pre-check: re-submitting returns 409, which is
  // the same answer the check would have given, so the extra round trip only
  // adds a way to be wrong. 409 also covers the other legitimate "nothing to
  // submit" case — this marketing version was already approved, after which
  // Apple releases later builds of it without a fresh review.
  const { status, body } = await client.request(
    'POST',
    '/v1/betaAppReviewSubmissions',
    {
      data: {
        type: 'betaAppReviewSubmissions',
        relationships: { build: { data: { type: 'builds', id: build.id } } },
      },
    },
    { allowStatuses: [409] },
  );

  if (status === 409) {
    console.log(
      `Beta app review submission returned 409 (${formatAscErrors(body) || 'no detail'}). ` +
        'Treating as already submitted or not required for this version.',
    );
    return;
  }

  console.log('Submitted for beta app review.');
}

async function main() {
  const keyId = requireEnv('IOS_ASC_KEY_ID');
  const issuerId = requireEnv('IOS_ASC_ISSUER_ID');
  const apiKeyPath = requireEnv('IOS_ASC_API_KEY_PATH');
  const groupName = requireEnv('IOS_BETA_GROUP_NAME');
  const buildVersion = requireEnv('IOS_BUILD_VERSION');
  const marketingVersion = requireEnv('IOS_MARKETING_VERSION');
  const bundleId = process.env.IOS_BUNDLE_ID?.trim() || DEFAULT_BUNDLE_ID;
  const whatsNew =
    process.env.IOS_WHATS_NEW?.trim() || `Build ${marketingVersion} (${buildVersion})`;
  const timeoutMs =
    Number(process.env.IOS_PROCESSING_TIMEOUT_SECONDS || DEFAULT_PROCESSING_TIMEOUT_SECONDS) * 1000;

  const client = new AppStoreConnectClient({
    privateKeyPem: await readFile(apiKeyPath, 'utf8'),
    keyId,
    issuerId,
  });

  const appId = await resolveAppId(client, bundleId);
  const { group, groups } = await resolveBetaGroup(client, { appId, groupName });
  const build = await waitForProcessedBuild(client, {
    appId,
    buildVersion,
    marketingVersion,
    timeoutMs,
  });

  // Free, and fails the run, so it goes before anything that mutates state.
  assertExportCompliance(build);

  const exclusive = process.env.IOS_EXCLUSIVE_BETA_GROUP === 'true';

  // Pruned here as well as at the end. A group with "automatically distribute
  // new builds" claims the build the moment it becomes distributable — which,
  // when Info.plist already carries the compliance answer, is the instant
  // processing completes, i.e. before any of the work below. That setting is
  // fixed when the group is created and is read-only in the API, so this
  // cleanup is the only lever; running it first shortens the window in which a
  // stray group holds a build pointed at the wrong API.
  if (exclusive) {
    await enforceExclusiveGroup(client, {
      build,
      group,
      groups,
      appId,
      buildVersion,
      marketingVersion,
    });
  }

  await setWhatToTest(client, { build, whatsNew });
  await addBuildToGroup(client, { group, build });
  await verifyGroupMembership(client, { appId, group, build, buildVersion, marketingVersion });

  if (group.isInternal) {
    console.log('Internal group: no beta app review needed.');
  } else {
    await submitForBetaReview(client, { build });
  }

  if (exclusive) {
    await enforceExclusiveGroup(client, {
      build,
      group,
      groups,
      appId,
      buildVersion,
      marketingVersion,
    });
  }

  console.log(
    `Done. Build ${marketingVersion} (${buildVersion}) is distributed to "${groupName}".`,
  );
}

main().catch((error) => {
  console.error(error.message ?? error);
  process.exit(1);
});
