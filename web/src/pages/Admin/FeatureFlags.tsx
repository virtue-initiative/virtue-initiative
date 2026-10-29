import { useEffect, useRef, useState } from 'preact/hooks';
import {
  Badge,
  Button,
  Card,
  Dialog,
  DialogActions,
  DialogHeader,
  Field,
  Input,
  Select,
  useToast,
} from '@virtueinitiative/shared-web';
import { FeatureFlag } from '../../utils/api/api';
import { api, describeError } from '../../utils/api';
import { usePromise } from '../../hooks/usePromise';
import { formatRelativeTimestamp } from '../../utils/time';

// Mirrors featureFlagKeySchema; the API rejects anything else with a 400.
const FLAG_KEY_PATTERN = '[a-z0-9_]{1,64}';

function isForbidden(err: unknown) {
  return (
    typeof err === 'object' &&
    err !== null &&
    'status' in err &&
    (err as { status?: unknown }).status === 403
  );
}

function parsePercent(text: string) {
  const value = Number(text);
  return Number.isInteger(value) && value >= 0 && value <= 100 ? value : null;
}

function rolloutLabel(percent: number) {
  if (percent === 0) return 'Overrides only';
  if (percent === 100) return 'Everyone';
  return `${percent}% of users`;
}

// api/SPEC.md API-061 to API-065.
export function FeatureFlagsSection() {
  const { push: pushToast } = useToast();
  const [flags, setFlags] = useState<FeatureFlag[] | null>(null);

  useEffect(() => {
    api
      .getAdminFlags()
      .then(setFlags)
      .catch((err: unknown) => {
        if (isForbidden(err)) return;
        const message = describeError(err, 'Failed to load feature flags');
        if (message) pushToast(message, 'error');
      });
  }, [pushToast]);

  function replaceFlag(flag: FeatureFlag) {
    setFlags((current) =>
      [...(current ?? []).filter((f) => f.key !== flag.key), flag].sort((a, b) =>
        a.key.localeCompare(b.key),
      ),
    );
  }

  function removeFlag(key: string) {
    setFlags((current) => (current ?? []).filter((f) => f.key !== key));
  }

  return (
    <section class="dashboard-section">
      <div class="dashboard-section-header">
        <h2>Feature flags</h2>
      </div>
      <p class="invite-desc">
        A flag is on for its rollout percentage of users, picked from each user's ID so the same
        users stay in as the percentage grows. An override turns the flag on or off for one user
        whatever the percentage. Changes reach the web app within a minute and devices at their next
        upload.
      </p>

      <NewFlagForm existing={flags ?? []} onSaved={replaceFlag} />

      {flags === null ? (
        <p class="loading">Loading…</p>
      ) : flags.length === 0 ? (
        <p class="empty">No feature flags yet.</p>
      ) : (
        <div class="admin-flag-list">
          {flags.map((flag) => (
            <FlagCard key={flag.key} flag={flag} onSaved={replaceFlag} onDeleted={removeFlag} />
          ))}
        </div>
      )}
    </section>
  );
}

function NewFlagForm({
  existing,
  onSaved,
}: {
  existing: FeatureFlag[];
  onSaved: (flag: FeatureFlag) => void;
}) {
  const { push: pushToast } = useToast();
  const [key, setKey] = useState('');
  const [percentText, setPercentText] = useState('0');
  const [description, setDescription] = useState('');
  const [saving, trackSave] = usePromise();
  const percent = parsePercent(percentText);

  function handleSubmit(e: Event) {
    e.preventDefault();
    if (percent === null) return;
    if (existing.some((flag) => flag.key === key)) {
      pushToast(`A flag named ${key} already exists.`, 'error');
      return;
    }
    trackSave(
      api
        .putAdminFlag(key, { rollout_percent: percent, description: description.trim() || null })
        .then((flag) => {
          onSaved(flag);
          setKey('');
          setPercentText('0');
          setDescription('');
          pushToast(`Created ${flag.key}.`, 'success');
        })
        .catch((err: unknown) => {
          const message = describeError(err, 'Failed to create flag');
          if (message) pushToast(message, 'error');
        }),
    );
  }

  return (
    <form class="admin-flag-new" onSubmit={handleSubmit}>
      <Field label="New flag key" id="admin-flag-key">
        <Input
          id="admin-flag-key"
          value={key}
          onInput={(e) => setKey((e.target as HTMLInputElement).value)}
          pattern={FLAG_KEY_PATTERN}
          title="Lowercase letters, digits and underscores."
          placeholder="new_report_tab"
          spellcheck={false}
          required
        />
      </Field>
      <Field label="Rollout %" id="admin-flag-percent">
        <Input
          id="admin-flag-percent"
          type="number"
          min={0}
          max={100}
          step={1}
          value={percentText}
          onInput={(e) => setPercentText((e.target as HTMLInputElement).value)}
          class="admin-flag-percent"
          required
        />
      </Field>
      <Field label="Description" id="admin-flag-description" class="admin-flag-description-field">
        <Input
          id="admin-flag-description"
          value={description}
          onInput={(e) => setDescription((e.target as HTMLInputElement).value)}
          maxLength={500}
        />
      </Field>
      <Button variant="primary" type="submit" disabled={saving || !key || percent === null}>
        {saving ? 'Creating…' : 'Create flag'}
      </Button>
    </form>
  );
}

function FlagCard({
  flag,
  onSaved,
  onDeleted,
}: {
  flag: FeatureFlag;
  onSaved: (flag: FeatureFlag) => void;
  onDeleted: (key: string) => void;
}) {
  const { push: pushToast } = useToast();
  const [percentText, setPercentText] = useState(String(flag.rollout_percent));
  const [description, setDescription] = useState(flag.description ?? '');
  const [overrideEmail, setOverrideEmail] = useState('');
  const [overrideValue, setOverrideValue] = useState<'on' | 'off'>('on');
  const [busy, trackBusy] = usePromise();
  const deleteDialogRef = useRef<HTMLDialogElement>(null);
  const percent = parsePercent(percentText);
  const dirty =
    percent !== flag.rollout_percent || (description.trim() || null) !== flag.description;

  useEffect(() => {
    setPercentText(String(flag.rollout_percent));
    setDescription(flag.description ?? '');
  }, [flag.rollout_percent, flag.description]);

  function track(promise: Promise<unknown>, failure: string) {
    trackBusy(
      promise.catch((err: unknown) => {
        const message = describeError(err, failure);
        if (message) pushToast(message, 'error');
      }),
    );
  }

  function handleSave(e: Event) {
    e.preventDefault();
    if (percent === null) return;
    track(
      api
        .putAdminFlag(flag.key, {
          rollout_percent: percent,
          description: description.trim() || null,
        })
        .then((saved) => {
          onSaved(saved);
          pushToast(`Saved ${saved.key}.`, 'success');
        }),
      'Failed to save flag',
    );
  }

  function setOverride(email: string, enabled: boolean | null) {
    track(
      api.setAdminFlagOverride(flag.key, { email, enabled }).then((saved) => {
        onSaved(saved);
        if (enabled !== null) setOverrideEmail('');
      }),
      'Failed to set override',
    );
  }

  function handleDelete() {
    track(
      api.deleteAdminFlag(flag.key).then(() => {
        deleteDialogRef.current?.close();
        onDeleted(flag.key);
        pushToast(`Deleted ${flag.key}.`, 'success');
      }),
      'Failed to delete flag',
    );
  }

  return (
    <Card class="admin-flag">
      <div class="admin-flag-header">
        <h3 class="admin-flag-key">{flag.key}</h3>
        <Badge variant={flag.rollout_percent === 0 ? 'gray' : 'green'}>
          {rolloutLabel(flag.rollout_percent)}
        </Badge>
        <span class="admin-flag-updated">Updated {formatRelativeTimestamp(flag.updated_at)}.</span>
      </div>

      <form class="admin-flag-row" onSubmit={handleSave}>
        <Field label="Rollout %" id={`flag-${flag.key}-percent`}>
          <Input
            id={`flag-${flag.key}-percent`}
            type="number"
            min={0}
            max={100}
            step={1}
            value={percentText}
            onInput={(e) => setPercentText((e.target as HTMLInputElement).value)}
            class="admin-flag-percent"
            required
          />
        </Field>
        <Field
          label="Description"
          id={`flag-${flag.key}-description`}
          class="admin-flag-description-field"
        >
          <Input
            id={`flag-${flag.key}-description`}
            value={description}
            onInput={(e) => setDescription((e.target as HTMLInputElement).value)}
            maxLength={500}
          />
        </Field>
        <Button variant="primary" type="submit" disabled={busy || !dirty || percent === null}>
          Save
        </Button>
      </form>

      <div class="admin-flag-overrides">
        <h4 class="admin-flag-subheading">Overrides</h4>
        {flag.overrides.length === 0 ? (
          <p class="admin-flag-none">No user overrides.</p>
        ) : (
          <ul class="admin-flag-override-list">
            {flag.overrides.map((override) => (
              <li key={override.user_id} class="admin-flag-override">
                <span class="admin-flag-override-email">{override.email}</span>
                <Badge variant={override.enabled ? 'green' : 'red'}>
                  {override.enabled ? 'On' : 'Off'}
                </Badge>
                <Button
                  variant="ghost"
                  type="button"
                  disabled={busy}
                  onClick={() => setOverride(override.email, null)}
                  aria-label={`Remove override for ${override.email}`}
                >
                  Remove
                </Button>
              </li>
            ))}
          </ul>
        )}
        <form
          class="admin-flag-row"
          onSubmit={(e) => {
            e.preventDefault();
            setOverride(overrideEmail.trim(), overrideValue === 'on');
          }}
        >
          <Field label="User email" id={`flag-${flag.key}-email`}>
            <Input
              id={`flag-${flag.key}-email`}
              type="email"
              value={overrideEmail}
              onInput={(e) => setOverrideEmail((e.target as HTMLInputElement).value)}
              required
            />
          </Field>
          <Field label="Override" id={`flag-${flag.key}-value`}>
            <Select
              id={`flag-${flag.key}-value`}
              value={overrideValue}
              onChange={(e) =>
                setOverrideValue((e.target as HTMLSelectElement).value as 'on' | 'off')
              }
            >
              <option value="on">On</option>
              <option value="off">Off</option>
            </Select>
          </Field>
          <Button variant="ghost" type="submit" disabled={busy || !overrideEmail.trim()}>
            Set override
          </Button>
        </form>
      </div>

      <div class="admin-flag-footer">
        <Button
          variant="danger"
          type="button"
          disabled={busy}
          onClick={() => deleteDialogRef.current?.showModal()}
        >
          Delete flag
        </Button>
      </div>

      <Dialog dialogRef={deleteDialogRef}>
        <DialogHeader>Delete flag</DialogHeader>
        <p class="invite-desc">
          Delete {flag.key}? It turns off for everyone, and its overrides are removed.
        </p>
        <DialogActions>
          <Button
            variant="ghost"
            type="button"
            disabled={busy}
            onClick={() => deleteDialogRef.current?.close()}
          >
            Cancel
          </Button>
          <Button variant="danger" type="button" disabled={busy} onClick={handleDelete}>
            {busy ? 'Deleting…' : 'Delete flag'}
          </Button>
        </DialogActions>
      </Dialog>
    </Card>
  );
}
