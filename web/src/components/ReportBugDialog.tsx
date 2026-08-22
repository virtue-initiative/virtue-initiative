import { useState } from 'preact/hooks';
import {
  Button,
  Dialog,
  DialogActions,
  DialogHeader,
  Field,
  Input,
  Textarea,
  useToast,
} from '@virtueinitiative/shared-web';
import { api } from '../utils/api';

type ReportBugDialogProps = {
  dialogRef: { current: HTMLDialogElement | null };
};

export function ReportBugDialog({ dialogRef }: ReportBugDialogProps) {
  const [message, setMessage] = useState('');
  const [contactEmail, setContactEmail] = useState('');
  const [loading, setLoading] = useState(false);
  const { push: pushToast } = useToast();

  function close() {
    dialogRef.current?.close();
  }

  function handleClose() {
    setMessage('');
    setContactEmail('');
  }

  async function handleSubmit(e: Event) {
    e.preventDefault();
    setLoading(true);
    try {
      await api.reportBug({
        message,
        contact_email: contactEmail.trim() || undefined,
        platform: 'web',
      });
      pushToast('Thanks for the report — we’ll take a look.', 'success');
      close();
    } catch (err) {
      pushToast(err instanceof Error ? err.message : 'Failed to send report', 'error');
    } finally {
      setLoading(false);
    }
  }

  return (
    <Dialog dialogRef={dialogRef} onClose={handleClose}>
      <DialogHeader>Report a bug</DialogHeader>
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
        <DialogActions>
          <Button variant="ghost" type="button" onClick={close}>
            Cancel
          </Button>
          <Button variant="primary" type="submit" disabled={loading || !message.trim()}>
            {loading ? 'Sending…' : 'Send report'}
          </Button>
        </DialogActions>
      </form>
    </Dialog>
  );
}
