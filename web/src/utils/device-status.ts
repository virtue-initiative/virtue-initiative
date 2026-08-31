import type { Device } from './api';

/** Human-readable label for a device's status, as shown on the devices list. */
export function deviceStatusLabel(status: Device['status']): string {
  switch (status) {
    case 'online':
      return 'Online';
    case 'logged_out':
      return 'Deactivated';
    default:
      return 'Offline';
  }
}

/** Badge variant matching a device's status: deactivated reads as a problem, not a lull. */
export function deviceStatusVariant(status: Device['status']): 'green' | 'red' | 'gray' {
  switch (status) {
    case 'online':
      return 'green';
    case 'logged_out':
      return 'red';
    default:
      return 'gray';
  }
}
