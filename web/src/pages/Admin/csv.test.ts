import { describe, expect, it } from 'vitest';
import { csvFilename, toCsv } from './csv';

describe('toCsv', () => {
  it('writes a header row and escapes per RFC 4180', () => {
    const csv = toCsv({
      columns: ['email', 'name', 'n', 'blob'],
      rows: [
        ['a@example.com', 'Plain', 1, null],
        ['b@example.com', 'Has, comma', 2.5, undefined],
        ['c@example.com', 'Says "hi"', 0, { k: 'v' }],
        ['d@example.com', 'Two\nlines', -1, true],
      ],
    });

    expect(csv).toBe(
      [
        'email,name,n,blob',
        'a@example.com,Plain,1,',
        'b@example.com,"Has, comma",2.5,',
        'c@example.com,"Says ""hi""",0,"{""k"":""v""}"',
        'd@example.com,"Two\nlines",-1,true',
        '',
      ].join('\r\n'),
    );
  });

  it('produces just a header for an empty result', () => {
    expect(toCsv({ columns: ['n'], rows: [] })).toBe('n\r\n');
  });
});

describe('csvFilename', () => {
  it('slugifies the title and appends the date', () => {
    const now = new Date('2026-09-18T15:00:00Z');
    expect(csvFilename('Signups by day', now)).toBe('signups-by-day-2026-09-18.csv');
    expect(csvFilename('Raw SQL', now)).toBe('raw-sql-2026-09-18.csv');
    expect(csvFilename('???', now)).toBe('query-2026-09-18.csv');
  });
});
