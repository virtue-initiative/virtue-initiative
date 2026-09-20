import type { AdminQueryResult } from '../../utils/api/api';

// Spreadsheets run a cell starting with one of these as a formula. Result
// values come from user-controlled data (emails, names, device names), so a
// leading quote keeps them as text. Numbers are left alone: -1 is not a formula.
const FORMULA_PREFIX = /^[=+\-@\t\r]/;

function csvCell(value: unknown): string {
  if (value === null || value === undefined) return '';
  let text = typeof value === 'object' ? JSON.stringify(value) : String(value);
  if (typeof value === 'string' && FORMULA_PREFIX.test(text)) {
    text = `'${text}`;
  }
  // RFC 4180: quote anything holding a comma, quote or line break; double inner quotes.
  return /[",\r\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

export function toCsv(result: Pick<AdminQueryResult, 'columns' | 'rows'>): string {
  const lines = [result.columns, ...result.rows].map((row) => row.map(csvCell).join(','));
  return lines.join('\r\n') + '\r\n';
}

// "Signups by day" -> "signups-by-day-2026-09-18.csv"
export function csvFilename(title: string, now = new Date()): string {
  const slug =
    title
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '') || 'query';
  return `${slug}-${now.toISOString().slice(0, 10)}.csv`;
}

export function downloadCsv(filename: string, csv: string) {
  const blob = new Blob([csv], { type: 'text/csv;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}
