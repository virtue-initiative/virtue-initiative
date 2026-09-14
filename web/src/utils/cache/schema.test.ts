import { describe, expect, it } from 'vitest';
import { CACHE_SCHEMA_VERSION, CACHE_TABLES, findSchemaDrift } from './schema';

function currentSchema(): Map<string, Set<string>> {
  return new Map(CACHE_TABLES.map((t) => [t.name, new Set(t.columns)]));
}

describe('findSchemaDrift', () => {
  it('reports no drift for a database matching the declaration', () => {
    expect(findSchemaDrift(CACHE_SCHEMA_VERSION, currentSchema())).toBeNull();
  });

  it('reports no drift for a brand-new database with no tables', () => {
    // A fresh database reads user_version 0 because that is SQLite's default, not because
    // it is stale. Reporting drift here would make every first run, and every run right
    // after the cache is cleared, log a rebuild it does not need.
    expect(findSchemaDrift(0, new Map())).toBeNull();
  });

  it('reports drift for a populated database left at user_version 0', () => {
    // The real upgrade case: tables created before this module existed, so the version was
    // never stamped and the columns may predate later additions.
    const existing = currentSchema();
    existing.get('events')!.delete('created_at');
    expect(findSchemaDrift(0, existing)).toContain('schema version 0');
  });

  it('reports drift when user_version is behind', () => {
    const reason = findSchemaDrift(0, currentSchema());
    expect(reason).toContain('schema version 0');
  });

  it('reports drift when user_version is ahead', () => {
    const reason = findSchemaDrift(CACHE_SCHEMA_VERSION + 1, currentSchema());
    expect(reason).toContain(`does not match expected ${CACHE_SCHEMA_VERSION}`);
  });

  it('reports drift when a declared table is missing', () => {
    const existing = currentSchema();
    existing.delete('events');
    expect(findSchemaDrift(CACHE_SCHEMA_VERSION, existing)).toBe('table "events" is missing');
  });

  it('reports drift when a declared column is missing', () => {
    const existing = currentSchema();
    existing.get('events')!.delete('created_at');
    expect(findSchemaDrift(CACHE_SCHEMA_VERSION, existing)).toBe(
      'table "events" is missing column "created_at"',
    );
  });

  it('ignores extra tables and columns the declaration does not name', () => {
    const existing = currentSchema();
    existing.set('direct_logs', new Set(['id']));
    existing.get('batches')!.add('legacy_column');
    expect(findSchemaDrift(CACHE_SCHEMA_VERSION, existing)).toBeNull();
  });

  it('declares a column list matching each table DDL', () => {
    for (const table of CACHE_TABLES) {
      const body = table.ddl.slice(table.ddl.indexOf('(') + 1, table.ddl.lastIndexOf(')'));
      const declared = body
        .split('\n')
        .map((line) => line.trim())
        .filter((line) => line.length > 0 && !line.startsWith('PRIMARY KEY'))
        .map((line) => line.split(/\s+/)[0]);
      expect(declared).toEqual(table.columns);
    }
  });
});
