import { render } from '@testing-library/preact';
import { describe, expect, it } from 'vitest';
import { DataLog } from '../../utils/api/api';
import { LogIcon } from './log-icons';
import { getLogCategory, getLogHelpAnchor, getLogHelpUrl, LOG_TYPES } from './shared';

function log(type: string, data: Record<string, unknown> = {}): DataLog {
  return { id: 'x', device_id: 'd', ts: 0, created_at: 0, type, data };
}

describe('getLogCategory — titles', () => {
  it('uses the improved, human-readable titles', () => {
    expect(getLogCategory(log('screenshot'))).toBe('Screenshot');
    expect(getLogCategory(log('lifecycle', { kind: 'computer_booted' }))).toBe('Computer Started');
    expect(getLogCategory(log('lifecycle', { kind: 'login' }))).toBe('Signed In');
    expect(getLogCategory(log('lifecycle', { kind: 'logout' }))).toBe('Signed Out');
    expect(getLogCategory(log('lifecycle', { kind: 'process_started' }))).toBe(
      'Monitoring Started',
    );
    expect(getLogCategory(log('lifecycle_alert', { reason: 'ping_gap_while_running' }))).toBe(
      'Unexpected Gap',
    );
    expect(getLogCategory(log('capture_failed'))).toBe('Capture Failed');
  });
});

describe('getLogHelpAnchor / getLogHelpUrl', () => {
  it('slugifies the category title into an anchor', () => {
    expect(getLogHelpAnchor(log('lifecycle_alert', { reason: 'ping_gap_while_running' }))).toBe(
      'unexpected-gap',
    );
    expect(
      getLogHelpAnchor(log('lifecycle_alert', { reason: 'force_killed_before_shutdown' })),
    ).toBe('process-force-stopped');
    expect(getLogHelpAnchor(log('screenshot'))).toBe('screenshot');
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
});
