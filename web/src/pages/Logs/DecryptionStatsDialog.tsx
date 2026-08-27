import { useRef, useState } from 'preact/hooks';
import { Alert, Dialog, DialogHeader } from '@virtueinitiative/shared-web';
import { cacheClient, type DecryptionStats } from '../../utils/cache/client';
import { InfoIcon } from '../../components/icons';
import { LANDING_URL } from '../../utils/landing-url';

const DECRYPTION_ERRORS_HELP_URL = `${LANDING_URL}/help/web/decryption-errors`;

type DecryptionStatsButtonProps = {
  viewerId: string;
  targetUserId: string;
  deviceId?: string;
  startTime: number;
  endTime: number;
};

function StatsSection({ title, stats }: { title: string; stats: DecryptionStats | null }) {
  if (!stats) return null;
  return (
    <div class="logs-stats-section">
      <h3 class="logs-stats-section-title">{title}</h3>
      <dl class="logs-stats-rows">
        <div class="logs-stats-row">
          <dt>Batches decrypted</dt>
          <dd>
            {stats.decryptedBatches} / {stats.totalBatches}
          </dd>
        </div>
        {stats.failedBatches > 0 && stats.failureReasons.length > 0 ? (
          <details class="logs-stats-failures-details">
            <summary class="logs-stats-row">
              <span class="logs-stats-row-start">
                <dt>Batches that failed to decrypt</dt>
                <span class="logs-stats-toggle" aria-hidden="true" />
              </span>
              <dd>{stats.failedBatches}</dd>
            </summary>
            <Alert variant="warning" class="logs-stats-failures">
              <ul class="logs-stats-failure-list">
                {stats.failureReasons.map((r) => (
                  <li key={r.error}>
                    <span class="logs-stats-failure-message">
                      {r.error} <span class="logs-stats-failure-count">×{r.count}</span>
                    </span>
                    <a
                      class="logs-stats-failure-help"
                      href={DECRYPTION_ERRORS_HELP_URL}
                      target="_blank"
                      rel="noreferrer"
                    >
                      Why?
                    </a>
                  </li>
                ))}
              </ul>
            </Alert>
          </details>
        ) : (
          <div class="logs-stats-row">
            <dt>Batches that failed to decrypt</dt>
            <dd>{stats.failedBatches}</dd>
          </div>
        )}
        <div class="logs-stats-row">
          <dt>Total events</dt>
          <dd>{stats.totalEvents}</dd>
        </div>
        <div class="logs-stats-row">
          <dt>Total screenshots</dt>
          <dd>{stats.totalScreenshots}</dd>
        </div>
      </dl>
    </div>
  );
}

// Self-contained icon button + dialog: the button triggers the two DecryptionStats fetches
// (all-time and current-filter) before opening, so the dialog never renders stale numbers.
export function DecryptionStatsButton({
  viewerId,
  targetUserId,
  deviceId,
  startTime,
  endTime,
}: DecryptionStatsButtonProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [globalStats, setGlobalStats] = useState<DecryptionStats | null>(null);
  const [filteredStats, setFilteredStats] = useState<DecryptionStats | null>(null);
  const [loading, setLoading] = useState(false);

  async function open() {
    if (!cacheClient || !viewerId || !targetUserId) return;
    setLoading(true);
    setGlobalStats(null);
    setFilteredStats(null);
    dialogRef.current?.showModal();
    try {
      const [global, filtered] = await Promise.all([
        cacheClient.getDecryptionStats(viewerId, targetUserId),
        cacheClient.getDecryptionStats(viewerId, targetUserId, deviceId, startTime, endTime),
      ]);
      setGlobalStats(global);
      setFilteredStats(filtered);
    } finally {
      setLoading(false);
    }
  }

  return (
    <>
      <button type="button" class="logs-stats-trigger" aria-label="Decryption stats" onClick={open}>
        <InfoIcon />
      </button>
      <Dialog dialogRef={dialogRef} class="logs-stats-dialog">
        <DialogHeader>Decryption stats</DialogHeader>
        {loading ? (
          <p class="logs-stats-loading">Loading…</p>
        ) : (
          <>
            <StatsSection title="All time, all devices" stats={globalStats} />
            <StatsSection title="Current filter" stats={filteredStats} />
          </>
        )}
      </Dialog>
    </>
  );
}
