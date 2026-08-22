import { useRef, useState } from 'preact/hooks';
import {
  Alert,
  Button,
  Dialog,
  DialogActions,
  DialogHeader,
  Field,
  Input,
  Textarea,
} from '@virtueinitiative/shared-web';
import { API_URL } from '../lib/api-url';

export function ReportBugButton() {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [message, setMessage] = useState('');
  const [contactEmail, setContactEmail] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sent, setSent] = useState(false);

  function open() {
    setMessage('');
    setContactEmail('');
    setError(null);
    setSent(false);
    dialogRef.current?.showModal();
  }

  function close() {
    dialogRef.current?.close();
  }

  async function handleSubmit(e: Event) {
    e.preventDefault();
    setError(null);
    setSubmitting(true);
    try {
      const form = new FormData();
      form.set(
        'metadata',
        JSON.stringify({
          message,
          contact_email: contactEmail.trim() || undefined,
          platform: 'web',
        }),
      );
      const response = await fetch(`${API_URL}/bug-report`, { method: 'POST', body: form });
      if (!response.ok) {
        throw new Error(`Report failed (${response.status})`);
      }
      setSent(true);
    } catch (err) {
      console.error(err);
      setError('Something went wrong sending your report. Please try again.');
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <>
      <button type="button" class="footer-link-button" onClick={open}>
        Report a Bug
      </button>
      <Dialog dialogRef={dialogRef}>
        <DialogHeader>Report a bug</DialogHeader>
        {sent ? (
          <>
            <p>Thanks for the report — we’ll take a look.</p>
            <DialogActions>
              <Button variant="primary" type="button" onClick={close}>
                Close
              </Button>
            </DialogActions>
          </>
        ) : (
          <form onSubmit={handleSubmit}>
            <Field label="What went wrong?">
              <Textarea
                value={message}
                onInput={(e) => setMessage((e.target as HTMLTextAreaElement).value)}
                placeholder="Describe what happened and what you expected instead."
                rows={5}
                required
                autoFocus
              />
            </Field>
            <Field label="Contact email (optional)" helpText="In case we need more details.">
              <Input
                type="email"
                value={contactEmail}
                onInput={(e) => setContactEmail((e.target as HTMLInputElement).value)}
                placeholder="you@example.com"
              />
            </Field>
            {error && <Alert variant="error">{error}</Alert>}
            <DialogActions>
              <Button variant="ghost" type="button" onClick={close}>
                Cancel
              </Button>
              <Button variant="primary" type="submit" disabled={submitting || !message.trim()}>
                {submitting ? 'Sending…' : 'Send report'}
              </Button>
            </DialogActions>
          </form>
        )}
      </Dialog>
    </>
  );
}
