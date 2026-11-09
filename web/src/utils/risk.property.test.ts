// Property tests for the concern level shown for a risk score (WEB-024).
import { describe, expect, it } from 'vitest';
import fc from 'fast-check';
import { describeRiskLevel, getRiskLevel } from '@virtueinitiative/shared-web/risk';

type Level = 'low' | 'medium' | 'high' | 'alert';
const RANK: Record<Level, number> = { low: 0, medium: 1, high: 2, alert: 3 };

// The table of WEB-024, on the score rounded to a whole percentage.
function levelOfPercent(percent: number): Level {
  if (percent < 40) return 'low';
  if (percent < 70) return 'medium';
  if (percent < 90) return 'high';
  return 'alert';
}

const scoreArb = fc.double({ min: 0, max: 1, noNaN: true });

describe('concern level (WEB-024)', () => {
  it('is judged on the score rounded to a whole percentage', () => {
    fc.assert(
      fc.property(scoreArb, (score) => {
        expect(getRiskLevel(score)).toBe(levelOfPercent(Math.round(score * 100)));
      }),
      { numRuns: 500 },
    );
  });

  it('agrees with the percentage shown beside it', () => {
    fc.assert(
      fc.property(scoreArb, (score) => {
        const percent = Math.round(score * 100);
        const label = levelOfPercent(percent);
        expect(describeRiskLevel(score)).toBe(
          `${percent}% concern (${label[0]!.toUpperCase()}${label.slice(1)})`,
        );
      }),
      { numRuns: 500 },
    );
  });

  it('never gives a lower level to a higher score', () => {
    fc.assert(
      fc.property(scoreArb, scoreArb, (a, b) => {
        const [low, high] = a <= b ? [a, b] : [b, a];
        expect(RANK[getRiskLevel(high)]).toBeGreaterThanOrEqual(RANK[getRiskLevel(low)]);
      }),
      { numRuns: 500 },
    );
  });

  it('shows nothing for a missing score', () => {
    expect(describeRiskLevel(null)).toBeNull();
    expect(describeRiskLevel(undefined)).toBeNull();
    expect(getRiskLevel(null)).toBe('low');
  });
});
