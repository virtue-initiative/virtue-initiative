import { useEffect, useRef, useState } from 'preact/hooks';
import {
  Badge,
  Button,
  Card,
  Checkbox,
  Dialog,
  DialogActions,
  DialogHeader,
  Field,
  Input,
  Select,
  Textarea,
  useToast,
} from '@virtueinitiative/shared-web';
import {
  ANNOUNCEMENT_MESSAGE_MAX_LENGTH,
  announcementPlatforms,
  type AnnouncementLevel,
  type AnnouncementPlatform,
} from '@virtueinitiative/shared-web/types';
import { Announcement } from '../../utils/api/api';
import { api, describeError } from '../../utils/api';
import { usePromise } from '../../hooks/usePromise';
import { formatRelativeTimestamp } from '../../utils/time';
import { isForbidden } from './forbidden';

const LEVEL_LABELS: Record<AnnouncementLevel, string> = {
  info: 'Info',
  warning: 'Warning',
  urgent: 'Urgent',
};

const LEVEL_BADGES = { info: 'gray', warning: 'yellow', urgent: 'red' } as const;

const PLATFORM_LABELS: Record<AnnouncementPlatform, string> = {
  linux: 'Linux',
  macos: 'macOS',
  windows: 'Windows',
  android: 'Android',
  ios: 'iOS',
};

// Mirrors createAnnouncementSchema's below_version; the API rejects anything else.
const VERSION_PATTERN = '\\d+(\\.\\d+)*';

function platformsLabel(platforms: string[] | null) {
  if (platforms === null) return 'All platforms';
  return platforms.map((p) => PLATFORM_LABELS[p as AnnouncementPlatform] ?? p).join(', ');
}

// web/SPEC.md WEB-072; api/SPEC.md API-068 to API-071.
export function AnnouncementsSection() {
  const { push: pushToast } = useToast();
  const [announcements, setAnnouncements] = useState<Announcement[] | null>(null);

  useEffect(() => {
    api
      .getAdminAnnouncements()
      .then(setAnnouncements)
      .catch((err: unknown) => {
        if (isForbidden(err)) return;
        const message = describeError(err, 'Failed to load announcements');
        if (message) pushToast(message, 'error');
      });
  }, [pushToast]);

  return (
    <section class="dashboard-section">
      <div class="dashboard-section-header">
        <h2>Announcements</h2>
      </div>
      <p class="invite-desc">
        Each device shows the newest announcement that matches its platform and version, below the
        buttons on its main screen. Devices pick up a change when they next fetch their settings,
        about once an hour.
      </p>

      <NewAnnouncementForm
        onCreated={(created) => setAnnouncements((current) => [created, ...(current ?? [])])}
      />

      {announcements === null ? (
        <p class="loading">Loading…</p>
      ) : announcements.length === 0 ? (
        <p class="empty">No announcements. Devices show nothing.</p>
      ) : (
        <div class="admin-flag-list">
          {announcements.map((announcement) => (
            <AnnouncementCard
              key={announcement.id}
              announcement={announcement}
              onDeleted={(id) =>
                setAnnouncements((current) => (current ?? []).filter((a) => a.id !== id))
              }
            />
          ))}
        </div>
      )}
    </section>
  );
}

function NewAnnouncementForm({ onCreated }: { onCreated: (announcement: Announcement) => void }) {
  const { push: pushToast } = useToast();
  const [message, setMessage] = useState('');
  const [url, setUrl] = useState('');
  const [level, setLevel] = useState<AnnouncementLevel>('info');
  const [allPlatforms, setAllPlatforms] = useState(true);
  const [platforms, setPlatforms] = useState<AnnouncementPlatform[]>([]);
  const [belowVersion, setBelowVersion] = useState('');
  const [saving, trackSave] = usePromise();
  const platformsValid = allPlatforms || platforms.length > 0;

  function togglePlatform(platform: AnnouncementPlatform, checked: boolean) {
    setPlatforms((current) =>
      announcementPlatforms.filter((p) => (p === platform ? checked : current.includes(p))),
    );
  }

  function handleSubmit(e: Event) {
    e.preventDefault();
    if (!message.trim() || !platformsValid) return;
    trackSave(
      api
        .createAdminAnnouncement({
          message: message.trim(),
          url: url.trim() || null,
          level,
          platforms: allPlatforms ? null : platforms,
          below_version: belowVersion.trim() || null,
        })
        .then((created) => {
          onCreated(created);
          setMessage('');
          setUrl('');
          setLevel('info');
          setAllPlatforms(true);
          setPlatforms([]);
          setBelowVersion('');
          pushToast('Announcement created.', 'success');
        })
        .catch((err: unknown) => {
          const text = describeError(err, 'Failed to create announcement');
          if (text) pushToast(text, 'error');
        }),
    );
  }

  return (
    <form class="admin-announcement-new" onSubmit={handleSubmit}>
      <Field label="Message" id="admin-announcement-message">
        <Textarea
          id="admin-announcement-message"
          value={message}
          onInput={(e) => setMessage((e.target as HTMLTextAreaElement).value)}
          maxLength={ANNOUNCEMENT_MESSAGE_MAX_LENGTH}
          rows={2}
          required
        />
      </Field>
      <div class="admin-flag-row">
        <Field
          label="Link (optional)"
          id="admin-announcement-url"
          class="admin-flag-description-field"
        >
          <Input
            id="admin-announcement-url"
            type="url"
            pattern="https://.*"
            title="A link starting with https://"
            placeholder="https://virtueinitiative.org/download"
            value={url}
            onInput={(e) => setUrl((e.target as HTMLInputElement).value)}
          />
        </Field>
        <Field label="Level" id="admin-announcement-level">
          <Select
            id="admin-announcement-level"
            value={level}
            onChange={(e) => setLevel((e.target as HTMLSelectElement).value as AnnouncementLevel)}
          >
            <option value="info">Info (small link)</option>
            <option value="warning">Warning (orange)</option>
            <option value="urgent">Urgent (red)</option>
          </Select>
        </Field>
        <Field label="Show only below version" id="admin-announcement-version">
          <Input
            id="admin-announcement-version"
            pattern={VERSION_PATTERN}
            title="Whole numbers separated by dots, such as 0.2.0."
            placeholder="Any version"
            value={belowVersion}
            onInput={(e) => setBelowVersion((e.target as HTMLInputElement).value)}
            class="admin-announcement-version"
          />
        </Field>
      </div>
      <fieldset class="admin-announcement-platforms">
        <legend>Platforms</legend>
        <Checkbox
          id="admin-announcement-all-platforms"
          label="All platforms"
          checked={allPlatforms}
          onChange={(e) => setAllPlatforms((e.target as HTMLInputElement).checked)}
        />
        {!allPlatforms &&
          announcementPlatforms.map((platform) => (
            <Checkbox
              key={platform}
              id={`admin-announcement-platform-${platform}`}
              label={PLATFORM_LABELS[platform]}
              checked={platforms.includes(platform)}
              onChange={(e) => togglePlatform(platform, (e.target as HTMLInputElement).checked)}
            />
          ))}
      </fieldset>
      <div>
        <Button
          variant="primary"
          type="submit"
          disabled={saving || !message.trim() || !platformsValid}
        >
          {saving ? 'Creating…' : 'Create announcement'}
        </Button>
      </div>
    </form>
  );
}

function AnnouncementCard({
  announcement,
  onDeleted,
}: {
  announcement: Announcement;
  onDeleted: (id: string) => void;
}) {
  const { push: pushToast } = useToast();
  const [busy, trackBusy] = usePromise();
  const deleteDialogRef = useRef<HTMLDialogElement>(null);

  function handleDelete() {
    trackBusy(
      api
        .deleteAdminAnnouncement(announcement.id)
        .then(() => {
          deleteDialogRef.current?.close();
          onDeleted(announcement.id);
          pushToast('Announcement deleted.', 'success');
        })
        .catch((err: unknown) => {
          const text = describeError(err, 'Failed to delete announcement');
          if (text) pushToast(text, 'error');
        }),
    );
  }

  return (
    <Card class="admin-flag">
      <div class="admin-flag-header">
        <Badge variant={LEVEL_BADGES[announcement.level]}>{LEVEL_LABELS[announcement.level]}</Badge>
        <span>{platformsLabel(announcement.platforms)}</span>
        <span>
          {announcement.below_version
            ? `Versions below ${announcement.below_version}`
            : 'All versions'}
        </span>
        <span class="admin-flag-updated">
          Created {formatRelativeTimestamp(announcement.created_at)}.
        </span>
      </div>
      <p class="admin-announcement-message">{announcement.message}</p>
      {announcement.url && (
        <a class="admin-announcement-url" href={announcement.url} target="_blank" rel="noreferrer">
          {announcement.url}
        </a>
      )}

      <div class="admin-flag-footer">
        <Button
          variant="danger"
          type="button"
          disabled={busy}
          onClick={() => deleteDialogRef.current?.showModal()}
        >
          Delete
        </Button>
      </div>

      <Dialog dialogRef={deleteDialogRef}>
        <DialogHeader>Delete announcement</DialogHeader>
        <p class="invite-desc">
          Delete this announcement? Devices stop showing it when they next fetch their settings.
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
            {busy ? 'Deleting…' : 'Delete announcement'}
          </Button>
        </DialogActions>
      </Dialog>
    </Card>
  );
}
