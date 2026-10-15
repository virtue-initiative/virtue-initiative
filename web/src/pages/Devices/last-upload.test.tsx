import { screen, waitFor } from '@testing-library/preact';
import { describe, expect, it, vi } from 'vitest';
import { renderWithClient } from '../../test-utils';
import { Devices } from './index';

const getDeviceLastUploads = vi.fn();

vi.mock('../../utils/cache', () => ({
  cacheClient: {
    setSession: vi.fn(),
    cacheQuery: vi.fn(),
    refetch: vi.fn(),
    clearCache: vi.fn().mockResolvedValue(undefined),
    deleteDeviceData: vi.fn().mockResolvedValue(undefined),
    getEventImage: vi.fn().mockResolvedValue(null),
    getDeviceBatchEndTimes: vi.fn().mockResolvedValue([]),
    getDeviceLastUploads: (...args: unknown[]) => getDeviceLastUploads(...args),
  },
}));

describe('Devices — last upload', () => {
  it("shows each device's last upload from the cache, and Never for a device without one", async () => {
    getDeviceLastUploads.mockResolvedValue({ 'device-1': Date.now() - 3 * 60 * 60 * 1000 });

    renderWithClient(<Devices />);

    await waitFor(() => expect(screen.getByText('3 hours ago')).toBeInTheDocument());
    expect(screen.getByText('Never')).toBeInTheDocument();
    expect(getDeviceLastUploads).toHaveBeenCalledWith('user-1', 'user-1');
  });
});
