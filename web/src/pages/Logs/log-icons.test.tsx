import { render } from '@testing-library/preact';
import { describe, expect, it } from 'vitest';
import { DataLog } from '../../utils/api/api';
import {
  getLogCategory,
  getLogHelpAnchor,
  getLogHelpUrl,
  getLogMessage,
  LOG_TYPES,
  LogIcon,
} from './shared';

function log(type: string, data: Record<string, unknown> = {}): DataLog {
  return { id: 'x', device_id: 'd', ts: 0, created_at: 0, type, data };
}

describe('getLogCategory — titles', () => {
  it('uses the improved, human-readable titles', () => {
    expect(getLogCategory(log('screenshot'))).toBe('Screenshot');
    expect(getLogCategory(log('lifecycle', { kind: 'system_login' }))).toBe('System Login');
    expect(getLogCategory(log('lifecycle', { kind: 'system_logout' }))).toBe('System Logout');
    expect(getLogCategory(log('lifecycle', { kind: 'suspend_detected' }))).toBe('Suspend Detected');
    expect(getLogCategory(log('lifecycle_alert', { reason: 'unexpected_gap' }))).toBe(
      'Unexpected Gap',
    );
    expect(getLogCategory(log('capture_failed'))).toBe('Capture Failed');
    expect(getLogCategory(log('screenshot_skipped'))).toBe('Screenshot Skipped');
    expect(getLogCategory(log('heartbeat'))).toBe('Heartbeat');
  });
});

describe('getLogMessage', () => {
  it('builds device-specific messages per log kind', () => {
    expect(getLogMessage(log('heartbeat'), 'Bob’s PC')).toBe('Heartbeat received from Bob’s PC');
    expect(getLogMessage(log('lifecycle', { kind: 'system_login' }), 'Bob’s PC')).toBe(
      'Bob’s PC was logged into or started up',
    );
    expect(getLogMessage(log('lifecycle', { kind: 'system_logout' }), 'Bob’s PC')).toBe(
      'Bob’s PC was logged out of or shut down',
    );
    expect(
      getLogMessage(
        log('lifecycle', { kind: 'suspend_detected', duration_ms: 90_000 }),
        'Bob’s PC',
      ),
    ).toContain('2 min');
  });
});

describe('getLogHelpAnchor / getLogHelpUrl', () => {
  it('slugifies the category title into an anchor', () => {
    expect(getLogHelpAnchor(log('lifecycle_alert', { reason: 'unexpected_gap' }))).toBe(
      'unexpected-gap',
    );
    expect(getLogHelpAnchor(log('lifecycle_alert', { reason: 'unexpected_stop' }))).toBe(
      'process-stopped-unexpectedly',
    );
    expect(getLogHelpAnchor(log('screenshot'))).toBe('screenshot');
    expect(getLogHelpAnchor(log('screenshot_skipped'))).toBe('screenshot-skipped');
  });

  it('builds a full help URL with the anchor', () => {
    expect(getLogHelpUrl(log('capture_failed'))).toBe(
      'https://virtueinitiative.org/help/web/log-types#capture-failed',
    );
  });
});

describe('LogIcon', () => {
  it('renders an SVG (not emoji text) for every log type', () => {
    for (const type of LOG_TYPES) {
      const { container, unmount } = render(<LogIcon log={log(type)} />);
      expect(container.querySelector('svg')).toBeTruthy();
      unmount();
    }
  });

  it('renders an SVG for every lifecycle kind and lifecycle_alert reason', () => {
    const samples: DataLog[] = [
      log('lifecycle', { kind: 'system_login' }),
      log('lifecycle', { kind: 'system_logout' }),
      log('lifecycle', { kind: 'suspend_detected' }),
      log('lifecycle_alert', { reason: 'unexpected_gap' }),
      log('lifecycle_alert', { reason: 'unexpected_stop' }),
      log('lifecycle_alert', { reason: 'unexpected_start' }),
      log('lifecycle_alert', { reason: 'user_stop' }),
      log('heartbeat'),
    ];
    for (const sample of samples) {
      const { container, unmount } = render(<LogIcon log={sample} />);
      expect(container.querySelector('svg')).toBeTruthy();
      unmount();
    }
  });
});
