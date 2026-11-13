import type { Reading } from '@/api/types';

// The output's threshold slider (ADR-0046): while it is dragged, the last
// run's explanation (ADR-0034) says what a new threshold would change, so
// that the run is redone only on release.

/** What a threshold would change against the last run: findings it adds and drops. */
export interface Change {
  added: number;
  dropped: number;
}

/**
 * Against the last run's readings, what holding every candidate to
 * `threshold` would change: a candidate below its threshold that reaches the
 * new one is added, a found one under it drops. Other verdicts (allowed, an
 * overlap, role words) do not depend on the threshold and are not counted.
 * An estimate: the run on release decides.
 */
export function change(readings: readonly Reading[], threshold: number): Change {
  const t = round(threshold);
  let added = 0;
  let dropped = 0;
  // A check reads what a reading before it read already: counted once.
  for (const reading of readings.filter((r) => !r.verify)) {
    for (const c of reading.candidates) {
      if (c.verdict === 'kept' && c.confidence < t) dropped++;
      else if (c.verdict === 'below_threshold' && c.confidence >= t) added++;
    }
  }
  return { added, dropped };
}

/** A confidence as a whole percentage: `0.7` is `70`. */
export function percent(x: number): number {
  return Math.round(x * 100);
}

/** As the engine compares: to three places, so that `0.7` is not `0.69999…`. */
function round(x: number): number {
  return Math.round(x * 1000) / 1000;
}
