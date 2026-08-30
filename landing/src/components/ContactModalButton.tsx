import { useRef } from 'preact/hooks';
import { Button, Dialog, DialogActions, DialogHeader } from '@virtueinitiative/shared-web';

type ContactModalButtonProps = {
  label: string;
  detail: string;
};

function MobileIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.7"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <path d="M7.5 3.75h9A1.5 1.5 0 0 1 18 5.25v13.5a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 6 18.75V5.25a1.5 1.5 0 0 1 1.5-1.5Z"></path>
      <path d="M10.5 17.25h3"></path>
    </svg>
  );
}

export function ContactModalButton({ label, detail }: ContactModalButtonProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);

  return (
    <>
      <button
        type="button"
        class="vi-btn vi-btn--outline text-icon download-button"
        onClick={() => dialogRef.current?.showModal()}
      >
        <span class="icon">
          <MobileIcon />
        </span>
        <span class="download-button-content">
          <span class="download-button-label">{label}</span>
          <span class="download-button-detail">{detail}</span>
        </span>
      </button>
      <Dialog dialogRef={dialogRef}>
        <DialogHeader>Contact us</DialogHeader>
        <p>
          The iOS app is currently available via TestFlight beta only. Contact us at{' '}
          <a href="mailto:help@virtueinitiative.org">help@virtueinitiative.org</a> to request
          access.
        </p>
        <DialogActions>
          <Button variant="primary" type="button" onClick={() => dialogRef.current?.close()}>
            Close
          </Button>
        </DialogActions>
      </Dialog>
    </>
  );
}
