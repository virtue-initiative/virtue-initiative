import { Badge } from '@virtueinitiative/shared-web';
import type { Device } from '../utils/api';
import {
  deviceStatusHelpUrl,
  deviceStatusLabel,
  deviceStatusVariant,
} from '../utils/device-status';
import './device-status-badge.css';

/**
 * A device's status tag, linking to the help page section that explains what
 * the status means. Rendered as a link rather than a button so it can sit
 * inside other interactive rows without nesting a control in a control.
 */
export function DeviceStatusBadge({
  status,
  class: className,
}: {
  status: Device['status'];
  class?: string;
}) {
  const label = deviceStatusLabel(status);

  return (
    <a
      class={['device-status-badge', className].filter(Boolean).join(' ')}
      href={deviceStatusHelpUrl(status)}
      target="_blank"
      rel="noreferrer"
      title={`What does "${label}" mean?`}
    >
      <Badge variant={deviceStatusVariant(status)}>{label}</Badge>
    </a>
  );
}
