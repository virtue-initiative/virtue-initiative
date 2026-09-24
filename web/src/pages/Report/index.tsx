import { useEffect, useMemo, useRef, useState } from 'preact/hooks';
import { SegmentedControl } from '@virtueinitiative/shared-web';
import { describeRiskLevel, getRiskLevel } from '@virtueinitiative/shared-web/risk';
import { LogQueryResult, useAPIContext, useDevices, usePartners } from '../../utils/api';
import { PageHeading } from '../../components/PageHeading';
import { ReportIcon } from '../../components/icons';
import { useUrlState } from '../../hooks/useUrlState';
import { EventImage, FeedLog, getLogCategory, LogDetailDialog, LogIcon } from '../Logs/shared';
import { ChevronLeftIcon, ChevronRightIcon } from '../Logs/log-icons';
import {
  buildReport,
  explainAlert,
  parseDateString,
  reportWindow,
  shiftDateString,
  toDateString,
  type ReportPeriod,
} from './report-model';
import { ScrollRow } from './ScrollRow';
import '../Logs/style.css';
import './style.css';

const PERIOD_SEGMENTS = [
  { label: 'Daily', value: 'day' },
  { label: 'Weekly', value: 'week' },
];

const RISK_BADGES = { alert: '⚠ Alert', high: '⚠ High', medium: 'Med' } as const;

function ordinal(n: number): string {
  const suffix = n % 100 >= 11 && n % 100 <= 13 ? 'th' : (['th', 'st', 'nd', 'rd'][n % 10] ?? 'th');
  return `${n}${suffix}`;
}

/** "September 24th, 2026" */
function longDate(date: string): string {
  const d = parseDateString(date);
  const month = d.toLocaleDateString(undefined, { month: 'long' });
  return `${month} ${ordinal(d.getDate())}, ${d.getFullYear()}`;
}

function shortDate(date: string): string {
  return parseDateString(date).toLocaleDateString(undefined, {
    weekday: 'short',
    month: 'short',
    day: 'numeric',
  });
}

function plural(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? '' : 's'}`;
}

/** "3 high risk alerts and 5 high risk screenshots", leaving out zero counts. */
function describeCounts(level: string, alerts: number, screenshots: number): string | null {
  const parts = [
    alerts > 0 && plural(alerts, `${level} risk alert`),
    screenshots > 0 && plural(screenshots, `${level} risk screenshot`),
  ].filter(Boolean);
  return parts.length ? parts.join(' and ') : null;
}

function RiskBadge({ log }: { log: FeedLog }) {
  const level = getRiskLevel(log.risk);
  if (level === 'low') return null;
  return (
    <span
      class={`logs-verify-badge ${level === 'medium' ? 'logs-verify-badge--moderate' : 'logs-verify-badge--failed'}`}
      title={describeRiskLevel(log.risk) ?? undefined}
    >
      {RISK_BADGES[level]}
    </span>
  );
}

export function Report({ userId: routeUserId }: { userId?: string }) {
  const api = useAPIContext();
  const viewerId = api?.userId ?? '';
  const targetUserId = routeUserId ?? viewerId;
  const isOwn = !routeUserId || routeUserId === viewerId;
  const { devices, loaded: devicesLoaded } = useDevices();
  const { watchings, loaded: partnersLoaded } = usePartners();

  const today = toDateString(new Date());
  const [period, setPeriod] = useUrlState<ReportPeriod>('period', 'string', 'day');
  const [rawDate, setDate] = useUrlState<string | null>('date', 'string', null);
  const date = rawDate && /^\d{4}-\d{2}-\d{2}$/.test(rawDate) && rawDate <= today ? rawDate : today;
  const range = reportWindow(period, date);

  const [result, setResult] = useState<LogQueryResult>({
    logs: [],
    complete: false,
    processed: 0,
    total: 0,
  });

  useEffect(() => {
    if (!api || !targetUserId) return;
    let cancelled = false;
    setResult(
      api.queryLogs(
        { userId: targetUserId, startTime: range.start, endTime: range.end },
        (next) => {
          if (!cancelled) setResult(next);
        },
      ),
    );
    return () => {
      cancelled = true;
    };
  }, [api, targetUserId, range.start, range.end]);

  const report = useMemo(() => buildReport(result.logs, range), [result.logs, range.start]);
  const [selected, setSelected] = useState<{ list: FeedLog[]; index: number } | null>(null);

  const partner = watchings.find((p) => p.user.id === targetUserId);
  const fullName = partner?.user.name ?? partner?.user.email ?? '';
  const firstName = partner?.user.name?.split(' ')[0] ?? fullName;
  const monitoredDevices = devices.filter(
    (d) => d.owner === targetUserId && d.status !== 'logged_out',
  );
  const deviceName = (id: string) => devices.find((d) => d.id === id)?.name ?? `${id.slice(0, 8)}…`;

  const title =
    !isOwn && !partnersLoaded ? 'Loading…' : isOwn ? 'My report' : `${fullName}'s report`;
  const isCurrent = date === today;
  const when =
    period === 'week'
      ? isCurrent
        ? 'this week'
        : 'that week'
      : isCurrent
        ? 'today'
        : date === shiftDateString(today, -1)
          ? 'yesterday'
          : `on ${shortDate(date)}`;
  const heading =
    period === 'week'
      ? `Activity report for the week of ${longDate(range.firstDate)}`
      : `${longDate(date)} activity report`;

  const highCounts = describeCounts(
    'high',
    report.high.alerts.length,
    report.high.screenshots.length,
  );
  const mediumTotal = report.medium.alerts.length + report.medium.screenshots.length;
  const nothingFlagged = !highCounts && mediumTotal === 0;
  const loading = !result.complete;

  const deviceSentence = isOwn
    ? `You have ${monitoredDevices.length ? plural(monitoredDevices.length, 'device') : 'no devices'} being monitored.`
    : `${firstName} has ${monitoredDevices.length ? plural(monitoredDevices.length, 'device') : 'no devices'} being monitored.`;
  const highSentence = highCounts
    ? `There ${/^1 /.test(highCounts) ? 'was' : 'were'} ${highCounts} ${when}.`
    : `There were no high risk alerts or screenshots ${when}.`;
  const mediumSentence =
    mediumTotal === 0
      ? 'There were no medium risk alerts or screenshots.'
      : `There ${mediumTotal === 1 ? 'was' : 'were'}${highCounts ? ' also' : ''} ${mediumTotal} medium risk ${mediumTotal === 1 ? 'alert or screenshot' : 'alerts and screenshots'}.`;

  const timeLabel = (ts: number) =>
    new Date(ts).toLocaleString(
      undefined,
      period === 'week'
        ? { weekday: 'short', hour: 'numeric', minute: '2-digit' }
        : { hour: 'numeric', minute: '2-digit' },
    );

  const open = (list: FeedLog[], index: number) => setSelected({ list, index });

  const dateLabel =
    period === 'week' ? `${shortDate(range.firstDate)} to ${shortDate(date)}` : shortDate(date);
  const dateInputRef = useRef<HTMLInputElement>(null);
  const openDatePicker = () => {
    const input = dateInputRef.current;
    if (!input) return;
    try {
      input.showPicker();
    } catch {
      // Older browsers without showPicker(): focusing still lets the date be typed.
      input.focus();
    }
  };

  const alertRow = (label: string, list: FeedLog[]) =>
    list.length > 0 && (
      <ScrollRow label={label} count={list.length}>
        {list.map((log, i) => (
          <button
            key={log.id}
            type="button"
            class={`report-alert-card report-alert-card--${getRiskLevel(log.risk)}`}
            onClick={() => open(list, i)}
          >
            <span class="report-alert-card-top">
              <span class="report-alert-card-icon">
                <LogIcon log={log} />
              </span>
              <span class="report-alert-card-title">{getLogCategory(log)}</span>
              <RiskBadge log={log} />
            </span>
            <span class="report-alert-card-body">{explainAlert(log, result.logs)}</span>
            <span class="report-meta">
              {deviceName(log.device_id)} · {timeLabel(log.ts)}
            </span>
          </button>
        ))}
      </ScrollRow>
    );

  const screenshotRow = (label: string, list: FeedLog[]) =>
    list.length > 0 && (
      <ScrollRow label={label} count={list.length}>
        {list.map((log, i) => (
          <figure
            key={log.id}
            class={`report-shot report-shot--${getRiskLevel(log.risk)}${(log.image_h ?? 0) > (log.image_w ?? 0) ? ' report-shot--portrait' : ''}`}
          >
            <div class="report-shot-frame">
              <span class="report-shot-badge">
                <RiskBadge log={log} />
              </span>
              <EventImage eventId={log.id} viewerId={viewerId} onClick={() => open(list, i)} />
            </div>
            <figcaption class="report-meta">
              {deviceName(log.device_id)} · {timeLabel(log.ts)}
            </figcaption>
          </figure>
        ))}
      </ScrollRow>
    );

  if (!isOwn && partnersLoaded && !partner) {
    return (
      <div class="dashboard report-page">
        <PageHeading icon={<ReportIcon />}>Report</PageHeading>
        <p class="report-lede">
          This report isn't available. You can only see reports for people you monitor.
        </p>
      </div>
    );
  }

  return (
    <div class="dashboard report-page">
      <PageHeading icon={<ReportIcon />}>{title}</PageHeading>

      <div class="report-toolbar">
        <SegmentedControl
          segments={PERIOD_SEGMENTS}
          value={period}
          onChange={(value) => setPeriod(value as ReportPeriod)}
        />
        <div class="report-date-nav">
          <button
            type="button"
            class="report-row-nav-button"
            aria-label={period === 'week' ? 'Previous week' : 'Previous day'}
            onClick={() => setDate(shiftDateString(date, period === 'week' ? -7 : -1))}
          >
            <ChevronLeftIcon />
          </button>
          <span class="report-date-center">
            <button
              type="button"
              class="report-date-button"
              aria-label={`Choose a date (showing ${dateLabel})`}
              onClick={openDatePicker}
            >
              {dateLabel}
            </button>
            {/* The native picker, opened by the button above. Visually hidden but
                kept in the layout so the browser anchors the picker to the date. */}
            <input
              ref={dateInputRef}
              class="report-date-input"
              type="date"
              tabIndex={-1}
              aria-hidden="true"
              value={date}
              max={today}
              onChange={(e) => {
                const picked = (e.target as HTMLInputElement).value;
                if (picked) setDate(picked >= today ? null : picked);
              }}
            />
          </span>
          <button
            type="button"
            class="report-row-nav-button"
            aria-label={period === 'week' ? 'Next week' : 'Next day'}
            disabled={isCurrent}
            onClick={() => {
              const next = shiftDateString(date, period === 'week' ? 7 : 1);
              setDate(next >= today ? null : next);
            }}
          >
            <ChevronRightIcon />
          </button>
        </div>
      </div>

      <article class="report">
        <h2 class="report-title">{heading}</h2>

        {loading && (
          <p class="report-syncing" role="status">
            {result.total > 0
              ? `Syncing logs (${result.processed} of ${result.total} blocks). Counts may still go up.`
              : 'Syncing logs.'}
          </p>
        )}

        <p class="report-lede">
          {devicesLoaded && deviceSentence} {!nothingFlagged && highSentence}
        </p>

        {alertRow('High risk alerts', report.high.alerts)}
        {screenshotRow('High risk screenshots', report.high.screenshots)}

        {!nothingFlagged && <p class="report-lede">{mediumSentence}</p>}

        {alertRow('Medium risk alerts', report.medium.alerts)}
        {screenshotRow('Medium risk screenshots', report.medium.screenshots)}

        {nothingFlagged && !loading && (
          <div class="report-all-clear">
            <p>
              Nothing concerning was flagged {when}.{' '}
              {report.screenshotCount > 0 &&
                `${plural(report.screenshotCount, 'screenshot')} ${report.screenshotCount === 1 ? 'was' : 'were'} captured, and none of them looked risky.`}
            </p>
            {!isOwn && <p>Consider sending {firstName} a note of encouragement.</p>}
          </div>
        )}

        {!nothingFlagged && (
          <p class="report-closing">
            {isOwn
              ? 'Your accountability partners see this same report. If something here needs explaining, it can help to reach out to them first.'
              : `If any of this activity looks questionable, reach out to ${firstName} and talk with them about it. Remember, the goal isn't to cause shame. It's to build them up and encourage them to avoid the things they've said they don't want to do.`}
          </p>
        )}

        <p class="report-footer">
          <a href={`${routeUserId ? `/logs/${routeUserId}` : '/logs'}?range=${period}`}>
            See every log in the Logs tab.
          </a>
        </p>
      </article>

      {selected && (
        <LogDetailDialog
          item={selected.list[selected.index]}
          deviceName={deviceName}
          viewerId={viewerId}
          onClose={() => setSelected(null)}
          onPrev={
            selected.index > 0
              ? () => setSelected({ ...selected, index: selected.index - 1 })
              : undefined
          }
          onNext={
            selected.index < selected.list.length - 1
              ? () => setSelected({ ...selected, index: selected.index + 1 })
              : undefined
          }
        />
      )}
    </div>
  );
}
