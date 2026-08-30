export const riskRatings = ['marginal', 'moderate', 'high', 'alert'] as const;

export type RiskRating = (typeof riskRatings)[number];

export const riskRatingBands = [
  { rating: 'marginal', minExclusive: 0, max: 0.4 },
  { rating: 'moderate', min: 0.4, max: 0.7 },
  { rating: 'high', min: 0.7, max: 0.9 },
  { rating: 'alert', min: 0.9, max: 1 },
] as const satisfies ReadonlyArray<{
  rating: RiskRating;
  min?: number;
  minExclusive?: number;
  max: number;
}>;

export function getRiskRating(risk: number | null | undefined): RiskRating | null {
  if (risk == null || Number.isNaN(risk) || risk <= 0 || risk > 1) {
    return null;
  }

  // Compare against the rounded percentage (not the raw float) so the rating
  // never disagrees with the percentage shown alongside it (e.g. a risk of
  // 0.8999999999999999 must not display as "90% concern (High)").
  const percentage = Math.round(risk * 100);

  if (percentage >= riskRatingBands[3].min * 100) {
    return 'alert';
  }

  if (percentage >= riskRatingBands[2].min * 100) {
    return 'high';
  }

  if (percentage >= riskRatingBands[1].min * 100) {
    return 'moderate';
  }

  return 'marginal';
}

export function hasTamperRisk(risk: number | null | undefined) {
  return getRiskRating(risk) !== null;
}

export function isHighRisk(risk: number | null | undefined) {
  return getRiskRating(risk) === 'high';
}

export function getRiskLevel(risk: number | null | undefined): 'alert' | 'high' | 'medium' | 'low' {
  const rating = getRiskRating(risk);
  if (rating === 'alert') return 'alert';
  if (rating === 'high') return 'high';
  if (rating === 'moderate') return 'medium';
  return 'low';
}

export function describeRiskLevel(risk: number | null | undefined): string | null {
  if (risk == null || typeof risk !== 'number' || Number.isNaN(risk)) {
    return null;
  }

  const percentage = Math.round(Math.max(0, Math.min(1, risk)) * 100);
  const level = getRiskLevel(risk);
  const label = level[0].toUpperCase() + level.slice(1);

  return `${percentage}% concern (${label})`;
}
