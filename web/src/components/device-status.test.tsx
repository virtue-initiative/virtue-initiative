// WEB-019: wherever a device is listed, its status is a coloured tag that links to the
// matching section of the device status help page.
import { screen, within } from '@testing-library/preact';
import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { describe, expect, it } from 'vitest';
import { server } from '../mocks/server';
import { TEST_DEVICES, TEST_WATCHING } from '../mocks/fixtures';
import { renderWithClient } from '../test-utils';
import { Devices } from '../pages/Devices/index';
import { Partners } from '../pages/Partners/index';
import type { Device } from '../utils/api';

const BASE = `http://localhost:8787/${CURRENT_API_VERSION}`;

const STATUSES: [Device['status'], string, string, string][] = [
  ['online', 'Online', 'green', '#online'],
  ['offline', 'Offline', 'gray', '#offline'],
  ['logged_out', 'Deactivated', 'red', '#deactivated'],
];

function devicesWithEveryStatus(owner: string): Device[] {
  return STATUSES.map(([status, label], i) => ({
    ...TEST_DEVICES[0],
    id: `device-${status}`,
    owner,
    name: `${label} device`,
    status,
    last_hash_at: Date.now() - (i + 1) * 60_000,
  }));
}

function expectStatusTag(label: string, colour: string, anchor: string) {
  const [tag] = screen.getAllByText(label);
  expect(tag).toHaveClass(`vi-badge--${colour}`);
  const link = tag.closest('a')!;
  expect(link.getAttribute('href')).toMatch(new RegExp(`/help/web/device-status${anchor}$`));
  expect(link).toHaveAttribute('target', '_blank');
}

describe('Device status tag (WEB-019)', () => {
  it.each(STATUSES)(
    'the Devices page shows %s as "%s", in %s, linking to its help section in a new tab (WEB-019)',
    async (_status, label, colour, anchor) => {
      server.use(
        http.get(`${BASE}/device`, () => HttpResponse.json(devicesWithEveryStatus('user-1'))),
      );
      renderWithClient(<Devices />);
      const card = (await screen.findByText(`${label} device`)).closest('.vi-card') as HTMLElement;
      const tag = within(card).getByText(label);
      expect(tag).toHaveClass(`vi-badge--${colour}`);
      const link = tag.closest('a')!;
      expect(link.getAttribute('href')).toMatch(new RegExp(`/help/web/device-status${anchor}$`));
      expect(link).toHaveAttribute('target', '_blank');
    },
  );

  it("the Partners page tags each of a watched person's devices the same way (WEB-019)", async () => {
    server.use(
      http.get(`${BASE}/device`, () =>
        HttpResponse.json(devicesWithEveryStatus(TEST_WATCHING.user.id)),
      ),
    );
    renderWithClient(<Partners />);
    await screen.findAllByText('Online device');
    for (const [, label, colour, anchor] of STATUSES) expectStatusTag(label, colour, anchor);
  });
});
