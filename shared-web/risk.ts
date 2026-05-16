export const riskRatings = ['marginal', 'moderate', 'high'] as const;

export type RiskRating = (typeof riskRatings)[number];

export const riskRatingBands = [
  { rating: 'marginal', minExclusive: 0, max: 0.4 },
  { rating: 'moderate', min: 0.4, max: 0.7 },
  { rating: 'high', min: 0.7, max: 1 },
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

  if (risk >= riskRatingBands[2].min) {
    return 'high';
  }

  if (risk >= riskRatingBands[1].min) {
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
