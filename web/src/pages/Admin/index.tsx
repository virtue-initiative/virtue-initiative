import { useEffect, useState } from 'preact/hooks';
import {
  AdminQueryPayload,
  AdminQueryPreset,
  AdminQueryResult,
  AdminQueryScanDetails,
  AnalyticsMetrics,
  AnalyticsSnapshot,
} from '../../utils/api/api';
import {
  ADMIN_QUERY_DEFAULT_LIMIT,
  ADMIN_QUERY_MAX_LIMIT,
  perPersonAverage,
} from '@virtueinitiative/shared-web/types';
import { api, describeError } from '../../utils/api';
import { usePromise } from '../../hooks/usePromise';
import { PageHeading } from '../../components/PageHeading';
import { ChartBarIcon } from '../../components/icons';
import { NotFound } from '../_404';
import {
  Alert,
  Button,
  Card,
  Field,
  Input,
  Select,
  Textarea,
  useToast,
} from '@virtueinitiative/shared-web';
import { formatRelativeTimestamp } from '../../utils/time';
import { csvFilename, downloadCsv, toCsv } from './csv';
import './style.css';

const HISTORY_DAYS = 90;

// api/SPEC.md API-051. Reachable only by URL; the API answers 403 for anyone
// not in the admins table, which this page turns into the ordinary 404 view.
export function Admin() {
  const { push: pushToast } = useToast();
  const [snapshots, setSnapshots] = useState<AnalyticsSnapshot[] | null>(null);
  const [forbidden, setForbidden] = useState(false);
  const [refreshing, trackRefresh] = usePromise();

  useEffect(() => {
    api
      .getAdminAnalytics(HISTORY_DAYS)
      .then(setSnapshots)
      .catch((err: unknown) => {
        if (isForbidden(err)) {
          setForbidden(true);
          return;
        }
        const message = describeError(err, 'Failed to load analytics');
        if (message) pushToast(message, 'error');
      });
  }, [pushToast]);

  function handleRefresh() {
    trackRefresh(
      api
        .refreshAdminAnalytics()
        .then((fresh) => {
          setSnapshots((current) => [
            fresh,
            ...(current ?? []).filter((snapshot) => snapshot.day !== fresh.day),
          ]);
          pushToast('Analytics refreshed', 'success');
        })
        .catch((err: unknown) => {
          const message = describeError(err, 'Failed to refresh analytics');
          if (message) pushToast(message, 'error');
        }),
    );
  }

  if (forbidden) {
    return <NotFound />;
  }

  const latest = snapshots?.[0] ?? null;

  return (
    <div class="dashboard admin-page">
      <PageHeading
        icon={<ChartBarIcon />}
        actions={
          <Button variant="primary" type="button" onClick={handleRefresh} disabled={refreshing}>
            {refreshing ? 'Refreshing…' : 'Refresh data'}
          </Button>
        }
      >
        Admin
      </PageHeading>

      <section class="dashboard-section">
        <div class="dashboard-section-header">
          <h2>Latest snapshot</h2>
          {latest && (
            <span class="admin-snapshot-time">
              Computed {formatRelativeTimestamp(latest.created_at)} for {latest.day}.
            </span>
          )}
        </div>
        {snapshots === null ? (
          <p class="loading">Loading…</p>
        ) : latest === null ? (
          <p class="empty">No snapshot yet. Refresh data to compute one now.</p>
        ) : (
          <StatTiles metrics={latest.metrics} />
        )}
      </section>

      <section class="dashboard-section">
        <div class="dashboard-section-header">
          <h2>History</h2>
        </div>
        {snapshots && snapshots.length > 0 ? (
          <HistoryTable snapshots={snapshots} />
        ) : (
          <p class="empty">One row appears here per day once snapshots exist.</p>
        )}
      </section>

      <QuerySection />
    </div>
  );
}

function isForbidden(err: unknown) {
  return (
    typeof err === 'object' &&
    err !== null &&
    'status' in err &&
    (err as { status?: unknown }).status === 403
  );
}

function devicesPerPerson(metrics: AnalyticsMetrics) {
  return perPersonAverage(metrics.devices.total, metrics.devices.owners);
}

function activeDevicesPerActiveUser(metrics: AnalyticsMetrics) {
  return perPersonAverage(metrics.active_devices.d7, metrics.active_users.d7);
}

function partnersPerPerson(metrics: AnalyticsMetrics) {
  return perPersonAverage(metrics.partners.accepted, metrics.partners.watched_users);
}

function StatTiles({ metrics }: { metrics: AnalyticsMetrics }) {
  const platforms = Object.entries(metrics.devices.by_platform).sort((a, b) => b[1] - a[1]);
  const tiles: { label: string; value: string; detail?: string }[] = [
    {
      label: 'Users',
      value: String(metrics.users.total),
      detail: `${metrics.users.verified} verified. New: ${metrics.users.new_1d} today, ${metrics.users.new_7d} this week, ${metrics.users.new_30d} this month.`,
    },
    {
      label: 'Active users',
      value: String(metrics.active_users.d7),
      detail: `Uploaded in the last 7 days. ${metrics.active_users.d1} in the last day, ${metrics.active_users.d30} in the last 30 days.`,
    },
    {
      label: 'Active devices',
      value: String(metrics.active_devices.d7),
      detail: `Uploaded in the last 7 days. ${metrics.active_devices.d1} in the last day, ${metrics.active_devices.d30} in the last 30 days. ${activeDevicesPerActiveUser(metrics)} per active user.`,
    },
    {
      label: 'Devices',
      value: String(metrics.devices.total),
      detail: `${devicesPerPerson(metrics)} per person with any.${
        platforms.length ? ` ${platforms.map(([p, n]) => `${p} ${n}`).join(', ')}.` : ''
      }`,
    },
    {
      label: 'Batches',
      value: String(metrics.batches.total),
      detail: `${metrics.batches.d1} in the last day, ${metrics.batches.d7} in the last 7 days.`,
    },
    {
      label: 'Partners',
      value: String(metrics.partners.accepted),
      detail: `Accepted. ${metrics.partners.pending} pending. ${partnersPerPerson(metrics)} per person with any.`,
    },
    {
      label: 'Locked passwords',
      value: String(metrics.locked_passwords.total),
      detail:
        'Entries stored and not deleted. Revealing one alerts every partner watching the owner.',
    },
  ];

  return (
    <div class="admin-stat-grid">
      {tiles.map((tile) => (
        <Card key={tile.label} class="admin-stat">
          <span class="admin-stat-label">{tile.label}</span>
          <span class="admin-stat-value">{tile.value}</span>
          {tile.detail && <span class="admin-stat-detail">{tile.detail}</span>}
        </Card>
      ))}
    </div>
  );
}

function HistoryTable({ snapshots }: { snapshots: AnalyticsSnapshot[] }) {
  return (
    <div class="admin-table-wrap">
      <table class="admin-table">
        <thead>
          <tr>
            <th>Day</th>
            <th>Users</th>
            <th>Active 7d</th>
            <th>Active devices 7d</th>
            <th>Active devices/active user</th>
            <th>Devices</th>
            <th>Devices/user</th>
            <th>Partners</th>
            <th>Partners/user</th>
            <th>Batches</th>
            <th>Locked passwords</th>
          </tr>
        </thead>
        <tbody>
          {snapshots.map((snapshot) => (
            <tr key={snapshot.day}>
              <td>{snapshot.day}</td>
              <td>{snapshot.metrics.users.total}</td>
              <td>{snapshot.metrics.active_users.d7}</td>
              <td>{snapshot.metrics.active_devices.d7}</td>
              <td>{activeDevicesPerActiveUser(snapshot.metrics)}</td>
              <td>{snapshot.metrics.devices.total}</td>
              <td>{devicesPerPerson(snapshot.metrics)}</td>
              <td>{snapshot.metrics.partners.accepted}</td>
              <td>{partnersPerPerson(snapshot.metrics)}</td>
              <td>{snapshot.metrics.batches.total}</td>
              <td>{snapshot.metrics.locked_passwords.total}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// Above this many rows read, a result gets a visible cost warning.
const EXPENSIVE_ROWS_READ = 50_000;

function scanDetails(err: unknown): AdminQueryScanDetails | null {
  if (typeof err !== 'object' || err === null || !('details' in err)) return null;
  const details = (err as { details?: unknown }).details;
  if (typeof details !== 'object' || details === null) return null;
  const candidate = details as Partial<AdminQueryScanDetails>;
  if (candidate.code !== 'full_scan') return null;
  return {
    code: 'full_scan',
    tables: Array.isArray(candidate.tables) ? candidate.tables : [],
    plan: Array.isArray(candidate.plan) ? candidate.plan : [],
  };
}

function QuerySection() {
  const { push: pushToast } = useToast();
  const [presets, setPresets] = useState<AdminQueryPreset[]>([]);
  const [preset, setPreset] = useState('');
  const [sql, setSql] = useState('');
  const [limitText, setLimitText] = useState(String(ADMIN_QUERY_DEFAULT_LIMIT));
  const parsedLimit = Number.parseInt(limitText, 10);
  const limit = Number.isFinite(parsedLimit)
    ? Math.min(Math.max(parsedLimit, 1), ADMIN_QUERY_MAX_LIMIT)
    : ADMIN_QUERY_DEFAULT_LIMIT;
  const [result, setResult] = useState<{ title: string; data: AdminQueryResult } | null>(null);
  const [scanWarning, setScanWarning] = useState<{
    title: string;
    payload: AdminQueryPayload;
    details: AdminQueryScanDetails;
  } | null>(null);
  const [running, trackRun] = usePromise();

  useEffect(() => {
    api
      .getAdminQueryPresets()
      .then((list) => {
        setPresets(list);
        setPreset((current) => current || list[0]?.name || '');
      })
      .catch((err: unknown) => {
        if (isForbidden(err)) return;
        const message = describeError(err, 'Failed to load queries');
        if (message) pushToast(message, 'error');
      });
  }, [pushToast]);

  function run(title: string, payload: AdminQueryPayload) {
    setScanWarning(null);
    trackRun(
      api
        .runAdminQuery({ ...payload, limit })
        .then((data) => setResult({ title, data }))
        .catch((err: unknown) => {
          const details = scanDetails(err);
          if (details) {
            setScanWarning({ title, payload, details });
            return;
          }
          const message = describeError(err, 'Query failed');
          if (message) pushToast(message, 'error');
        }),
    );
  }

  const selected = presets.find((p) => p.name === preset);

  return (
    <section class="dashboard-section">
      <div class="dashboard-section-header">
        <h2>Queries</h2>
      </div>
      <p class="invite-desc">
        Queries run live against the database. Every row the database reads is billed, so results
        show the rows read and raw SQL that would scan the whole batches table is held for
        confirmation. Credential columns are never returned.
      </p>

      <Field label="Row limit" id="admin-limit" class="admin-limit-field">
        <Input
          id="admin-limit"
          type="number"
          min={1}
          max={ADMIN_QUERY_MAX_LIMIT}
          value={limitText}
          onInput={(e) => setLimitText((e.target as HTMLInputElement).value)}
          class="admin-limit-input"
        />
      </Field>

      <form
        class="admin-query-form"
        onSubmit={(e) => {
          e.preventDefault();
          if (preset) run(selected?.label ?? preset, { preset });
        }}
      >
        <Field label="Preset query" id="admin-preset">
          <Select
            id="admin-preset"
            value={preset}
            onChange={(e) => setPreset((e.target as HTMLSelectElement).value)}
            disabled={presets.length === 0}
          >
            {presets.map((p) => (
              <option key={p.name} value={p.name}>
                {p.label}
              </option>
            ))}
          </Select>
        </Field>
        {selected && <p class="admin-preset-description">{selected.description}</p>}
        <Button variant="primary" type="submit" disabled={running || !preset}>
          Run preset
        </Button>
      </form>

      <form
        class="admin-query-form"
        onSubmit={(e) => {
          e.preventDefault();
          if (sql.trim()) run('Raw SQL', { sql });
        }}
      >
        <Field label="Raw SQL" id="admin-sql">
          <Textarea
            id="admin-sql"
            rows={5}
            value={sql}
            onInput={(e) => setSql((e.target as HTMLTextAreaElement).value)}
            placeholder="SELECT email, created_at FROM users ORDER BY created_at DESC"
            spellcheck={false}
          />
        </Field>
        <Button variant="primary" type="submit" disabled={running || !sql.trim()}>
          Run SQL
        </Button>
      </form>

      {scanWarning && (
        <div class="admin-scan-warning">
          <Alert variant="warning">
            <p class="admin-scan-warning-text">
              This query would read every row of {scanWarning.details.tables.join(', ')}. Each row
              read is billed. Narrow it with an indexed column such as created_at, or run it anyway.
            </p>
            <pre class="admin-plan">{scanWarning.details.plan.join('\n')}</pre>
            <Button
              variant="danger"
              type="button"
              disabled={running}
              onClick={() =>
                run(scanWarning.title, {
                  ...scanWarning.payload,
                  allow_scan: true,
                } as AdminQueryPayload)
              }
            >
              Run anyway
            </Button>
          </Alert>
        </div>
      )}

      {result && <ResultsTable title={result.title} result={result.data} limit={limit} />}
    </section>
  );
}

function formatCell(value: unknown) {
  if (value === null || value === undefined) return '';
  if (typeof value === 'object') return JSON.stringify(value);
  return String(value);
}

function ResultsTable({
  title,
  result,
  limit,
}: {
  title: string;
  result: AdminQueryResult;
  limit: number;
}) {
  return (
    <div class="admin-results">
      <div class="admin-results-header">
        <h3 class="admin-results-title">
          {title}: {result.rows.length} {result.rows.length === 1 ? 'row' : 'rows'} returned,{' '}
          {result.rows_read.toLocaleString()} rows read.
        </h3>
        <Button
          variant="ghost"
          type="button"
          disabled={result.rows.length === 0}
          onClick={() => downloadCsv(csvFilename(title), toCsv(result))}
        >
          Download CSV
        </Button>
      </div>
      {result.rows_read >= EXPENSIVE_ROWS_READ && (
        <Alert variant="warning">
          This query read {result.rows_read.toLocaleString()} rows. Add an indexed filter before
          running it again.
        </Alert>
      )}
      {result.truncated && (
        <Alert variant="info">
          Showing the first {limit} rows. Raise the row limit or narrow the query to see the rest.
        </Alert>
      )}
      {result.rows.length === 0 ? (
        <p class="empty">No rows.</p>
      ) : (
        <div class="admin-table-wrap">
          <table class="admin-table">
            <thead>
              <tr>
                {result.columns.map((column) => (
                  <th key={column}>{column}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {result.rows.map((row, i) => (
                <tr key={i}>
                  {row.map((cell, j) => (
                    <td key={j}>{formatCell(cell)}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
