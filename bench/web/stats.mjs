// Summary statistics shared by the benchmark runners. Bootstrap uses a fixed seed so the
// same raw data always produces the same report.

export const quantile = (sorted, q) => {
  if (!sorted.length) return NaN;
  const i = (sorted.length - 1) * q, lo = Math.floor(i), hi = Math.ceil(i);
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (i - lo);
};

export const median = xs => quantile([...xs].sort((a, b) => a - b), 0.5);

// 95% bootstrap confidence interval of the median.
export function medianCI(xs, rounds = 2000) {
  let seed = 0x9e3779b9;
  const rand = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 2 ** 32);
  const meds = [];
  for (let r = 0; r < rounds; r++) {
    const sample = xs.map(() => xs[Math.floor(rand() * xs.length)]);
    meds.push(median(sample));
  }
  meds.sort((a, b) => a - b);
  return [quantile(meds, 0.025), quantile(meds, 0.975)];
}

export function describe(xs) {
  const s = [...xs].sort((a, b) => a - b);
  return { n: s.length, p50: quantile(s, 0.5), p95: quantile(s, 0.95), p99: quantile(s, 0.99), ci95: medianCI(xs) };
}

export function shuffle(items, seed) {
  const a = [...items];
  for (let i = a.length - 1; i > 0; i--) {
    seed = (seed * 1664525 + 1013904223) >>> 0;
    const j = seed % (i + 1);
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}
