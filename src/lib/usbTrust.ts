export type TrustScoreTone = "danger" | "warning" | "success";

export function formatTrustScore(score: number): string {
  return `${clampTrustScore(score)}/100`;
}

export function trustScoreTone(score: number): TrustScoreTone {
  const clamped = clampTrustScore(score);

  if (clamped < 40) return "danger";
  if (clamped < 70) return "warning";
  return "success";
}

function clampTrustScore(score: number): number {
  if (!Number.isFinite(score)) return 0;
  return Math.min(100, Math.max(0, Math.round(score)));
}
