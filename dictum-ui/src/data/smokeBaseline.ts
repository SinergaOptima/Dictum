export type SmokeBaselineCategory = {
  category: string;
  runs: number;
  p50LatencyMs: number;
  p95LatencyMs: number;
  avgLatencyMs: number;
  missRate: number;
  placeholderRate: number;
  avgConfidence: number | null;
  avgSimilarityToExpected: number | null;
};

export type SmokeBaselineSummary = {
  fixturesDir: string;
  iterations: number;
  totalRuns: number;
  totalFiles: number;
  p50LatencyMs: number;
  p95LatencyMs: number;
  avgLatencyMs: number;
  missRate: number;
  placeholderRate: number;
  avgConfidence: number | null;
  avgSimilarityToExpected: number | null;
  categories: SmokeBaselineCategory[];
};

export const smokeBaseline: SmokeBaselineSummary = {
  fixturesDir: "benchmarks/fixtures",
  iterations: 1,
  totalRuns: 4,
  totalFiles: 4,
  p50LatencyMs: 22765.8497,
  p95LatencyMs: 27069.9318,
  avgLatencyMs: 17628.7173,
  missRate: 0,
  placeholderRate: 0,
  avgConfidence: 0.795,
  avgSimilarityToExpected: 0.76666665,
  categories: [
    {
      category: "long_form",
      runs: 1,
      p50LatencyMs: 27069.9318,
      p95LatencyMs: 27069.9318,
      avgLatencyMs: 27069.9318,
      missRate: 0,
      placeholderRate: 0,
      avgConfidence: 0.88,
      avgSimilarityToExpected: 1,
    },
    {
      category: "noisy_room",
      runs: 1,
      p50LatencyMs: 22765.8497,
      p95LatencyMs: 22765.8497,
      avgLatencyMs: 22765.8497,
      missRate: 0,
      placeholderRate: 0,
      avgConfidence: 0.88,
      avgSimilarityToExpected: null,
    },
    {
      category: "quiet_speech",
      runs: 1,
      p50LatencyMs: 10359.258300000001,
      p95LatencyMs: 10359.258300000001,
      avgLatencyMs: 10359.258300000001,
      missRate: 0,
      placeholderRate: 0,
      avgConfidence: 0.71999997,
      avgSimilarityToExpected: 0.3,
    },
    {
      category: "whisper_speech",
      runs: 1,
      p50LatencyMs: 10319.8294,
      p95LatencyMs: 10319.8294,
      avgLatencyMs: 10319.8294,
      missRate: 0,
      placeholderRate: 0,
      avgConfidence: 0.7,
      avgSimilarityToExpected: 1,
    },
  ],
};
