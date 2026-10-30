import { describe, expect, it } from 'vitest';
import { screen, waitFor } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { server } from '../mocks/server';
import { TEST_USER } from '../mocks/fixtures';
import { renderWithClient } from '../test-utils';
import { Sidebar } from './Sidebar';

const API = `http://localhost:8787/${CURRENT_API_VERSION}`;

type Report = { message: string; contact_email?: string; platform: string };

/** Records what the dialog sends to the bug report endpoint (API-042). */
function captureReports(respond: () => Response = () => new HttpResponse(null, { status: 204 })) {
  const reports: Report[] = [];
  server.use(
    http.post(`${API}/bug-report`, async ({ request }) => {
      const form = await request.formData();
      reports.push(JSON.parse(form.get('metadata') as string));
      return respond();
    }),
  );
  return reports;
}

async function openDialog() {
  const user = userEvent.setup();
  renderWithClient(<Sidebar />);
  await user.click(await screen.findByRole('button', { name: new RegExp(TEST_USER.name!) }));
  await user.click(screen.getByRole('button', { name: 'Report an issue' }));
  const dialog = screen.getByText('What went wrong?').closest('dialog')!;
  return { user, dialog };
}

const description = () => screen.getByPlaceholderText(/describe what happened/i);
const send = () => screen.getByRole('button', { name: /^(send report|sending…)$/i });

describe('Report an issue (WEB-064)', () => {
  it('opens from the account menu (WEB-064)', async () => {
    const { dialog } = await openDialog();
    expect(dialog).toHaveAttribute('open');
  });

  it('requires a description: the field is required and Send stays disabled until one is typed (WEB-064)', async () => {
    const { user } = await openDialog();
    expect(description()).toBeRequired();
    expect(send()).toBeDisabled();

    await user.type(description(), '   ');
    expect(send()).toBeDisabled();

    await user.type(description(), 'The page is blank');
    expect(send()).toBeEnabled();
  });

  it('offers an optional contact email, and says the account email is used when it is blank (WEB-064)', async () => {
    await openDialog();
    const contact = screen.getByPlaceholderText('you@example.com');
    expect(contact).not.toBeRequired();
    expect(
      screen.getByText(`Leave blank to use your account email (${TEST_USER.email}).`),
    ).toBeInTheDocument();
  });

  it('points users to the device app for problems with logs or screenshots (WEB-064)', async () => {
    await openDialog();
    expect(
      screen.getByText(/report it from the Virtue client app on that device instead/i),
    ).toBeInTheDocument();
  });

  it('sends the description and contact email, closes, and thanks the user in a toast (WEB-064)', async () => {
    const reports = captureReports();
    const { user, dialog } = await openDialog();
    await user.type(description(), 'Screenshots page is blank');
    await user.type(screen.getByPlaceholderText('you@example.com'), 'me@elsewhere.org');
    await user.click(send());

    expect(await screen.findByText(/Thanks for the report/)).toBeInTheDocument();
    expect(dialog).not.toHaveAttribute('open');
    expect(reports).toEqual([
      { message: 'Screenshots page is blank', contact_email: 'me@elsewhere.org', platform: 'web' },
    ]);
  });

  it('sends no contact email when it is left blank (WEB-064)', async () => {
    const reports = captureReports();
    const { user } = await openDialog();
    await user.type(description(), 'Something broke');
    await user.click(send());

    await screen.findByText(/Thanks for the report/);
    expect(reports).toHaveLength(1);
    expect(reports[0].contact_email).toBeUndefined();
  });

  it('disables Send and says "Sending…" while the report goes out (WEB-064, WEB-021)', async () => {
    let finish: () => void = () => {};
    const gate = new Promise<void>((resolve) => (finish = resolve));
    server.use(
      http.post(`${API}/bug-report`, async () => {
        await gate;
        return new HttpResponse(null, { status: 204 });
      }),
    );
    const { user } = await openDialog();
    await user.type(description(), 'Something broke');
    await user.click(send());

    await waitFor(() => expect(send()).toHaveTextContent('Sending…'));
    expect(send()).toBeDisabled();
    finish();
    await screen.findByText(/Thanks for the report/);
  });

  it('keeps the dialog open with what was typed, and shows the error, when sending fails (WEB-064, WEB-010)', async () => {
    captureReports(() => HttpResponse.json({ error: 'Too many reports' }, { status: 429 }));
    const { user, dialog } = await openDialog();
    await user.type(description(), 'Something broke');
    await user.click(send());

    expect(await screen.findByText('Too many reports')).toBeInTheDocument();
    expect(dialog).toHaveAttribute('open');
    expect(description()).toHaveValue('Something broke');
    expect(screen.queryByText(/Thanks for the report/)).not.toBeInTheDocument();
  });
});
