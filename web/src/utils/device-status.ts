import type { Device } from './api';
import { LANDING_URL } from './landing-url';

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

export const DEVICE_STATUS_HELP_URL = `${LANDING_URL}/help/web/device-status`;

/** Deep link to the section of the help page explaining this particular status.
 * The anchors mirror the ids markdown generates from that page's headings. */
export function deviceStatusHelpUrl(status: Device['status']): string {
  return `${DEVICE_STATUS_HELP_URL}#${deviceStatusLabel(status).toLowerCase()}`;
}
