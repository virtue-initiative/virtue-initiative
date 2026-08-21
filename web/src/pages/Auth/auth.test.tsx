import { render, screen, waitFor } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { LocationProvider } from 'preact-iso';
import { ToastProvider } from '@virtueinitiative/shared-web';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { http, HttpResponse } from 'msw';
import { describe, expect, it } from 'vitest';
import { server } from '../../mocks/server';
import { Auth } from './index';

const BASE = `http://localhost:8787/${CURRENT_API_VERSION}`;

function renderAuth(mode: 'login' | 'signup' | 'forgot-password' = 'login') {
  return render(
    <LocationProvider>
      <ToastProvider>
        <Auth mode={mode} />
      </ToastProvider>
    </LocationProvider>,
  );
}

describe('Auth — login', () => {
  it('renders email and password fields', () => {
    renderAuth('login');
    expect(screen.getByPlaceholderText('you@example.com')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('Enter your password')).toBeInTheDocument();
  });

  it('renders Log in and Sign up tabs in the segmented control', () => {
    renderAuth('login');
    // The tab area has "Log in" and "Sign up" buttons in a SegmentedControl
    const tabs = screen.getAllByRole('button', { name: /^(log in|sign up)$/i });
    const labels = tabs.map((b) => b.textContent?.toLowerCase().trim());
    expect(labels).toContain('log in');
    expect(labels).toContain('sign up');
  });

  it('shows an error when login fails', async () => {
    server.use(
      http.get(`${BASE}/user/login-material`, () =>
        HttpResponse.json({
          password_salt: btoa('testsalt12345678'),
          memory_cost_kib: 2048,
          time_cost: 1,
          parallelism: 1,
        }),
      ),
      http.post(`${BASE}/login`, () =>
        HttpResponse.json({ error: 'Invalid credentials' }, { status: 401 }),
      ),
    );

    const user = userEvent.setup();
    renderAuth('login');

    await user.type(screen.getByPlaceholderText('you@example.com'), 'bad@example.com');
    await user.type(screen.getByPlaceholderText('Enter your password'), 'wrongpassword');
    // Both the tab and form submit have "Log in" text; pick the submit button
    const logInButtons = screen.getAllByRole('button', { name: /^log in$/i });
    const submitBtn = logInButtons.find((b) => b.getAttribute('type') === 'submit')!;
    await user.click(submitBtn);

    await waitFor(() => {
      // An error Alert should appear
      const alerts = screen.getAllByRole('alert');
      const errorAlert = alerts.find((a) => a.className?.includes('error'));
      expect(errorAlert ?? alerts[alerts.length - 1]).toBeInTheDocument();
    });
  });
});

describe('Auth — signup', () => {
  it('renders email field for signup mode', () => {
    renderAuth('signup');
    expect(screen.getByPlaceholderText('you@example.com')).toBeInTheDocument();
  });

  it('shows "Check your email" dialog after signup request', async () => {
    const user = userEvent.setup();
    renderAuth('signup');

    await user.type(screen.getByPlaceholderText('you@example.com'), 'new@example.com');
    await user.click(screen.getByRole('button', { name: /send verification email/i }));

    await waitFor(() => {
      expect(screen.getByText('Check your email')).toBeInTheDocument();
    });
  });
});

describe('Auth — finish signup', () => {
  it('renders a disabled email field populated from the signup token', async () => {
    window.history.pushState({}, '', '/signup?signup_token=test-token');
    renderAuth('signup');

    await waitFor(() => {
      const emailInput = screen.getByPlaceholderText('you@example.com') as HTMLInputElement;
      expect(emailInput.value).toBe('test@example.com');
      expect(emailInput).toBeDisabled();
    });

    expect(screen.getByPlaceholderText('Choose a password')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('Retype your password')).toBeInTheDocument();

    window.history.pushState({}, '', '/');
  });
});

describe('Auth — forgot password', () => {
  it('renders the forgot password hint', () => {
    renderAuth('forgot-password');
    expect(
      screen.getByText(/enter your email to receive a password reset link/i),
    ).toBeInTheDocument();
  });

  it('shows a status message after submitting', async () => {
    const user = userEvent.setup();
    renderAuth('forgot-password');

    await user.type(screen.getByPlaceholderText('you@example.com'), 'test@example.com');
    await user.click(screen.getByRole('button', { name: /send reset link/i }));

    await waitFor(() => {
      expect(
        screen.getByText(/if that email exists, a reset link has been sent/i),
      ).toBeInTheDocument();
    });
  });
});
