import { useEffect, useMemo, useState } from 'preact/hooks';
import { Button, Checkbox, SegmentedControl } from '@virtueinitiative/shared-web';
import type { AnalyticsSnapshot } from '../../utils/api/api';
import {
  formatTrendDay,
  formatTrendValue,
  niceCeil,
  seriesFor,
  trendVariables,
  type TrendPoint,
  type TrendVariable,
} from './trends';

const HIDDEN_KEY = 'admin-trends-hidden';

function loadHidden(): Set<string> {
  try {
    const raw = window.localStorage.getItem(HIDDEN_KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    return new Set(Array.isArray(parsed) ? parsed.filter((k) => typeof k === 'string') : []);
  } catch {
    return new Set();
  }
}

function saveHidden(hidden: Set<string>) {
  try {
    window.localStorage.setItem(HIDDEN_KEY, JSON.stringify([...hidden]));
  } catch {
    // Per-viewer convenience only; losing it is fine.
  }
}

// Small multiples: every variable gets its own chart and its own y-axis, since
// the counts span single digits to thousands and one shared axis would flatten
// most of them. The x-axis and the hovered day are shared across all of them.
export function Trends({ snapshots }: { snapshots: AnalyticsSnapshot[] }) {
  const variables = useMemo(() => trendVariables(snapshots), [snapshots]);
  const [hidden, setHidden] = useState<Set<string>>(() =>
    typeof window === 'undefined' ? new Set() : loadHidden(),
  );
  const [view, setView] = useState<'chart' | 'table'>('chart');
  const [hoveredDay, setHoveredDay] = useState<string | null>(null);

  useEffect(() => saveHidden(hidden), [hidden]);

  const visible = variables.filter((v) => !hidden.has(v.key));
  const days = useMemo(
    () => [...snapshots].sort((a, b) => (a.day < b.day ? -1 : 1)).map((s) => s.day),
    [snapshots],
  );

  function toggle(key: string, show: boolean) {
    setHidden((current) => {
      const next = new Set(current);
      if (show) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  const groups = [...new Set(variables.map((v) => v.group))];

  return (
    <div class="admin-trends">
      <div class="admin-trends-controls">
        <div class="admin-trends-actions">
          <SegmentedControl
            segments={[
              { label: 'Charts', value: 'chart' },
              { label: 'Table', value: 'table' },
            ]}
            value={view}
            onChange={(value) => setView(value as 'chart' | 'table')}
          />
          <Button variant="ghost" type="button" onClick={() => setHidden(new Set())}>
            Show all
          </Button>
          <Button
            variant="ghost"
            type="button"
            onClick={() => setHidden(new Set(variables.map((v) => v.key)))}
          >
            Hide all
          </Button>
        </div>
        <div class="admin-trends-picker">
          {groups.map((group) => (
            <fieldset key={group} class="admin-trends-group">
              <legend>{group}</legend>
              {variables
                .filter((v) => v.group === group)
                .map((variable) => (
                  <Checkbox
                    key={variable.key}
                    label={variable.label}
                    checked={!hidden.has(variable.key)}
                    onChange={(e) => toggle(variable.key, (e.target as HTMLInputElement).checked)}
                  />
                ))}
            </fieldset>
          ))}
        </div>
      </div>

      {visible.length === 0 ? (
        <p class="empty">Every variable is hidden. Pick some above.</p>
      ) : view === 'table' ? (
        <TrendTable variables={visible} snapshots={snapshots} />
      ) : (
        <div class="admin-trends-grid">
          {visible.map((variable) => (
            <TrendChart
              key={variable.key}
              variable={variable}
              points={seriesFor(variable, snapshots)}
              days={days}
              hoveredDay={hoveredDay}
              onHover={setHoveredDay}
            />
          ))}
        </div>
      )}
    </div>
  );
}

const WIDTH = 300;
const HEIGHT = 110;
const PAD = { top: 8, right: 8, bottom: 20, left: 8 };
const PLOT_W = WIDTH - PAD.left - PAD.right;
const PLOT_H = HEIGHT - PAD.top - PAD.bottom;

function TrendChart({
  variable,
  points,
  days,
  hoveredDay,
  onHover,
}: {
  variable: TrendVariable;
  points: TrendPoint[];
  days: string[];
  hoveredDay: string | null;
  onHover: (day: string | null) => void;
}) {
  const first = points[0];
  const last = points[points.length - 1];
  const span = Math.max(last.t - first.t, 1);
  const yMax = niceCeil(Math.max(...points.map((p) => p.value)));

  const x = (t: number) => PAD.left + ((t - first.t) / span) * PLOT_W;
  const y = (value: number) => PAD.top + PLOT_H - (value / yMax) * PLOT_H;

  const linePath = points.map((p, i) => `${i === 0 ? 'M' : 'L'}${x(p.t)},${y(p.value)}`).join(' ');
  const areaPath = `${linePath} L${x(last.t)},${y(0)} L${x(first.t)},${y(0)} Z`;

  const hovered = hoveredDay ? points.find((p) => p.day === hoveredDay) : undefined;
  const readout = hovered ?? last;

  function nearestDay(clientX: number, target: SVGSVGElement) {
    const rect = target.getBoundingClientRect();
    const t = first.t + ((clientX - rect.left) / rect.width) * (WIDTH / PLOT_W) * span;
    let best = points[0];
    for (const p of points) {
      if (Math.abs(p.t - t) < Math.abs(best.t - t)) best = p;
    }
    return best.day;
  }

  function step(delta: number) {
    const index = days.indexOf(readout.day);
    const next = days[Math.min(Math.max(index + delta, 0), days.length - 1)];
    onHover(next);
  }

  return (
    <figure class="admin-trend" data-key={variable.key}>
      <figcaption class="admin-trend-caption">
        <span class="admin-trend-label">{variable.label}</span>
        <span class="admin-trend-readout">
          <strong>{formatTrendValue(readout.value)}</strong>
          <span class="admin-trend-readout-day">{formatTrendDay(readout.day)}</span>
        </span>
      </figcaption>
      <svg
        class="admin-trend-svg"
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        role="img"
        aria-label={`${variable.label}, ${formatTrendValue(last.value)} on ${formatTrendDay(last.day)}`}
        tabIndex={0}
        onPointerMove={(e) => onHover(nearestDay(e.clientX, e.currentTarget))}
        onPointerLeave={() => onHover(null)}
        onKeyDown={(e) => {
          if (e.key === 'ArrowLeft') step(-1);
          else if (e.key === 'ArrowRight') step(1);
          else if (e.key === 'Escape') onHover(null);
          else return;
          e.preventDefault();
        }}
      >
        {[0, 0.5, 1].map((fraction) => (
          <line
            key={fraction}
            class="admin-trend-grid"
            x1={PAD.left}
            x2={WIDTH - PAD.right}
            y1={y(yMax * fraction)}
            y2={y(yMax * fraction)}
          />
        ))}
        <text class="admin-trend-tick" x={PAD.left} y={PAD.top - 2}>
          {formatTrendValue(yMax)}
        </text>
        <text class="admin-trend-tick" x={PAD.left} y={HEIGHT - 6}>
          {formatTrendDay(first.day)}
        </text>
        <text class="admin-trend-tick" x={WIDTH - PAD.right} y={HEIGHT - 6} text-anchor="end">
          {formatTrendDay(last.day)}
        </text>
        <path class="admin-trend-area" d={areaPath} />
        <path class="admin-trend-line" d={linePath} vector-effect="non-scaling-stroke" />
        {hovered && (
          <>
            <line
              class="admin-trend-crosshair"
              x1={x(hovered.t)}
              x2={x(hovered.t)}
              y1={PAD.top}
              y2={PAD.top + PLOT_H}
            />
            <circle class="admin-trend-dot" cx={x(hovered.t)} cy={y(hovered.value)} r={4} />
          </>
        )}
        {!hovered && <circle class="admin-trend-dot" cx={x(last.t)} cy={y(last.value)} r={4} />}
      </svg>
    </figure>
  );
}

function TrendTable({
  variables,
  snapshots,
}: {
  variables: TrendVariable[];
  snapshots: AnalyticsSnapshot[];
}) {
  return (
    <div class="admin-table-wrap">
      <table class="admin-table">
        <thead>
          <tr>
            <th>Day</th>
            {variables.map((variable) => (
              <th key={variable.key}>{variable.label}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {snapshots.map((snapshot) => (
            <tr key={snapshot.day}>
              <td>{snapshot.day}</td>
              {variables.map((variable) => (
                <td key={variable.key}>{formatTrendValue(variable.read(snapshot.metrics))}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
