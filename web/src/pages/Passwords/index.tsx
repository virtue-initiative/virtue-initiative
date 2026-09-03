import { useMemo, useRef, useState } from 'preact/hooks';
import {
  LockedPassword,
  describeError,
  useAPIContext,
  usePasswords,
  useUser,
} from '../../utils/api';
import { decryptForOwnKey, encryptForPublicKey } from '../../utils/api/crypto';
import { PageHeading } from '../../components/PageHeading';
import { LockIcon } from '../../components/icons';
import { PasswordField } from '../Auth/PasswordField';
import {
  Badge,
  Button,
  Card,
  CardActions,
  CardGrid,
  CardHeader,
  Dialog,
  DialogActions,
  DialogHeader,
  Field,
  Input,
  useToast,
} from '@virtueinitiative/shared-web';
import { formatRelativeTimestamp } from '../../utils/time';
import './style.css';

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

export function Passwords() {
  const { passwords, loaded } = usePasswords();

  const active = useMemo(() => passwords.filter((p) => p.deleted_at === null), [passwords]);
  const deleted = useMemo(() => passwords.filter((p) => p.deleted_at !== null), [passwords]);

  return (
    <div class="dashboard">
      <PageHeading icon={<LockIcon />} actions={<AddPasswordButton />}>
        Passwords
      </PageHeading>
      <p class="invite-desc">
        A locked password is a secret only you can technically reach. Reading it permanently flags
        it and immediately notifies every partner watching you.
      </p>
      {!loaded ? (
        <p class="loading">Loading…</p>
      ) : active.length === 0 ? (
        <p class="empty">No locked passwords</p>
      ) : (
        <CardGrid>
          {active.map((password) => (
            <PasswordCard key={password.id} password={password} />
          ))}
        </CardGrid>
      )}

      {deleted.length > 0 && (
        <section class="dashboard-section">
          <div class="dashboard-section-header">
            <h2>Recently deleted</h2>
          </div>
          <CardGrid>
            {deleted.map((password) => (
              <DeletedPasswordCard key={password.id} password={password} />
            ))}
          </CardGrid>
        </section>
      )}
    </div>
  );
}

function AddPasswordButton() {
  const api = useAPIContext();
  const user = useUser();
  const [label, setLabel] = useState('');
  const [value, setValue] = useState('');
  const [saving, setSaving] = useState(false);
  const { push: pushToast } = useToast();
  const dialogRef = useRef<HTMLDialogElement>(null);

  function open() {
    setLabel('');
    setValue('');
    dialogRef.current?.showModal();
  }

  function close() {
    dialogRef.current?.close();
  }

  async function handleSubmit(e: Event) {
    e.preventDefault();
    if (!api || !user?.pub_key) {
      pushToast('Your account is not set up for locked passwords yet.', 'error');
      return;
    }

    setSaving(true);
    try {
      const wrapped = await encryptForPublicKey(
        Uint8Array.fromBase64(user.pub_key),
        textEncoder.encode(value),
      );
      await api.createPassword(label, wrapped.toBase64());
      close();
    } catch (err) {
      const message = describeError(err, 'Failed to create locked password');
      if (message) pushToast(message, 'error');
    } finally {
      setSaving(false);
    }
  }

  return (
    <>
      <Button variant="primary" type="button" onClick={open}>
        Add password
      </Button>
      <Dialog dialogRef={dialogRef}>
        <DialogHeader>Add a locked password</DialogHeader>
        <p class="invite-desc">
          Store a secret you don't want easy access to, like a Screen Time passcode. Reading it back
          permanently flags it and notifies your partners.
        </p>
        <form onSubmit={handleSubmit}>
          <Field label="Name" id="locked-password-label">
            <Input
              id="locked-password-label"
              type="text"
              value={label}
              onInput={(e) => setLabel((e.target as HTMLInputElement).value)}
              placeholder="Screen Time passcode"
              required
              autoFocus
            />
          </Field>
          <PasswordField
            label="Secret value"
            id="locked-password-value"
            value={value}
            onInput={(e) => setValue((e.target as HTMLInputElement).value)}
            required
          />
          <DialogActions>
            <Button variant="ghost" type="button" onClick={close} disabled={saving}>
              Cancel
            </Button>
            <Button variant="primary" type="submit" disabled={saving}>
              {saving ? 'Saving…' : 'Add password'}
            </Button>
          </DialogActions>
        </form>
      </Dialog>
    </>
  );
}

function PasswordCard({ password }: { password: LockedPassword }) {
  const api = useAPIContext();
  const [revealedValue, setRevealedValue] = useState<string | null>(null);
  const [revealing, setRevealing] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const { push: pushToast } = useToast();
  const confirmRevealRef = useRef<HTMLDialogElement>(null);
  const deleteDialogRef = useRef<HTMLDialogElement>(null);

  async function reveal() {
    if (!api?.session.privateKey) {
      pushToast('Your account is not set up for locked passwords yet.', 'error');
      return;
    }

    setRevealing(true);
    try {
      const { wrapped_value } = await api.revealPassword(password.id);
      const plainBytes = await decryptForOwnKey(
        api.session.privateKey,
        Uint8Array.fromBase64(wrapped_value),
      );
      setRevealedValue(textDecoder.decode(plainBytes));
    } catch (err) {
      const message = describeError(err, 'Failed to reveal password');
      if (message) pushToast(message, 'error');
    } finally {
      setRevealing(false);
    }
  }

  function handleRevealClick() {
    if (password.accessed_at === null) {
      confirmRevealRef.current?.showModal();
      return;
    }
    reveal().catch(() => {});
  }

  async function handleDeleteConfirmed() {
    setDeleting(true);
    try {
      await api?.removePassword(password.id);
      deleteDialogRef.current?.close();
    } catch (err) {
      const message = describeError(err, 'Failed to delete password');
      if (message) pushToast(message, 'error');
    } finally {
      setDeleting(false);
    }
  }

  return (
    <Card>
      <CardHeader>
        <span class="vi-card__name">{password.label}</span>
        {password.accessed_at !== null && <Badge variant="red">Accessed</Badge>}
      </CardHeader>
      <dl class="vi-card__meta">
        <dt>Added</dt>
        <dd>{formatRelativeTimestamp(password.created_at)}</dd>
        {password.accessed_at !== null && (
          <>
            <dt>Accessed</dt>
            <dd>{formatRelativeTimestamp(password.accessed_at)}</dd>
          </>
        )}
      </dl>
      {revealedValue !== null && (
        <p class="locked-password-value">
          <code>{revealedValue}</code>
        </p>
      )}
      <CardActions>
        {revealedValue === null ? (
          <Button variant="ghost" type="button" onClick={handleRevealClick} disabled={revealing}>
            {revealing ? 'Revealing…' : 'Reveal'}
          </Button>
        ) : (
          <Button variant="ghost" type="button" onClick={() => setRevealedValue(null)}>
            Hide
          </Button>
        )}
        <Button
          variant="danger"
          type="button"
          onClick={() => deleteDialogRef.current?.showModal()}
          disabled={deleting}
        >
          Delete
        </Button>
      </CardActions>

      <Dialog dialogRef={confirmRevealRef}>
        <DialogHeader>Reveal "{password.label}"?</DialogHeader>
        <p class="invite-desc">
          This permanently flags this password as accessed and immediately emails every partner
          watching you. This can't be undone.
        </p>
        <DialogActions>
          <Button
            variant="ghost"
            type="button"
            onClick={() => confirmRevealRef.current?.close()}
            disabled={revealing}
          >
            Cancel
          </Button>
          <Button
            variant="danger"
            type="button"
            onClick={() => {
              confirmRevealRef.current?.close();
              reveal().catch(() => {});
            }}
            disabled={revealing}
          >
            Reveal anyway
          </Button>
        </DialogActions>
      </Dialog>

      <Dialog dialogRef={deleteDialogRef}>
        <DialogHeader>Delete "{password.label}"?</DialogHeader>
        <p class="invite-desc">
          It moves to Recently deleted for 7 days, where you can restore it or delete it
          permanently.
        </p>
        <DialogActions>
          <Button
            variant="ghost"
            type="button"
            onClick={() => deleteDialogRef.current?.close()}
            disabled={deleting}
          >
            Cancel
          </Button>
          <Button
            variant="danger"
            type="button"
            onClick={() => handleDeleteConfirmed().catch(() => {})}
            disabled={deleting}
          >
            {deleting ? 'Deleting…' : 'Delete'}
          </Button>
        </DialogActions>
      </Dialog>
    </Card>
  );
}

function DeletedPasswordCard({ password }: { password: LockedPassword }) {
  const api = useAPIContext();
  const [restoring, setRestoring] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const { push: pushToast } = useToast();
  const deleteDialogRef = useRef<HTMLDialogElement>(null);

  async function handleRestore() {
    setRestoring(true);
    try {
      await api?.restorePassword(password.id);
    } catch (err) {
      const message = describeError(err, 'Failed to restore password');
      if (message) pushToast(message, 'error');
    } finally {
      setRestoring(false);
    }
  }

  async function handlePermanentDeleteConfirmed() {
    setDeleting(true);
    try {
      await api?.permanentlyDeletePassword(password.id);
      deleteDialogRef.current?.close();
    } catch (err) {
      const message = describeError(err, 'Failed to delete password');
      if (message) pushToast(message, 'error');
    } finally {
      setDeleting(false);
    }
  }

  return (
    <Card>
      <CardHeader>
        <span class="vi-card__name">{password.label}</span>
      </CardHeader>
      <dl class="vi-card__meta">
        <dt>Deleted</dt>
        <dd>{formatRelativeTimestamp(password.deleted_at)}</dd>
      </dl>
      <CardActions>
        <Button
          variant="ghost"
          type="button"
          onClick={() => handleRestore().catch(() => {})}
          disabled={restoring || deleting}
        >
          {restoring ? 'Restoring…' : 'Restore'}
        </Button>
        <Button
          variant="danger"
          type="button"
          onClick={() => deleteDialogRef.current?.showModal()}
          disabled={restoring || deleting}
        >
          Delete permanently
        </Button>
      </CardActions>

      <Dialog dialogRef={deleteDialogRef}>
        <DialogHeader>Delete "{password.label}" permanently?</DialogHeader>
        <p class="invite-desc">This can't be undone.</p>
        <DialogActions>
          <Button
            variant="ghost"
            type="button"
            onClick={() => deleteDialogRef.current?.close()}
            disabled={deleting}
          >
            Cancel
          </Button>
          <Button
            variant="danger"
            type="button"
            onClick={() => handlePermanentDeleteConfirmed().catch(() => {})}
            disabled={deleting}
          >
            {deleting ? 'Deleting…' : 'Delete permanently'}
          </Button>
        </DialogActions>
      </Dialog>
    </Card>
  );
}
