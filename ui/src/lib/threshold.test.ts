import { describe, expect, it } from 'vitest';
import type { Explained, Reading, Verdict } from '@/api/types';
import { change, percent } from './threshold';

function candidate(confidence: number, verdict: Verdict): Explained {
  return { start: 0, end: 1, value: 'x', entity: 'PERSON', detector: 'ner', confidence, threshold: 0.7, verdict };
}

const readings: Reading[] = [
  {
    text: 'synthetic',
    candidates: [
      candidate(1.0, 'kept'),
      candidate(0.8, 'kept'),
      candidate(0.7, 'kept'),
      candidate(0.6, 'below_threshold'),
      candidate(0.4, 'below_threshold'),
      candidate(0.9, 'allowed'),
      candidate(0.5, 'overlap'),
    ],
  },
];

describe('change', () => {
  it('counts nothing at the threshold the run used', () => {
    expect(change(readings, 0.7)).toEqual({ added: 0, dropped: 0 });
  });

  it('adds what reaches a lower threshold, and only below-threshold candidates', () => {
    expect(change(readings, 0.6)).toEqual({ added: 1, dropped: 0 });
    expect(change(readings, 0.1)).toEqual({ added: 2, dropped: 0 });
  });

  it('drops what falls under a higher one, never a finding at 1.0 within the range', () => {
    expect(change(readings, 0.85)).toEqual({ added: 0, dropped: 2 });
    expect(change(readings, 0.95)).toEqual({ added: 0, dropped: 2 });
  });

  it('compares as the engine does, not in binary fractions', () => {
    // 0.3 + 0.4 is 0.7000000000000001: a finding at 0.7 stays.
    expect(change([{ text: '', candidates: [candidate(0.7, 'kept')] }], 0.3 + 0.4)).toEqual({
      added: 0,
      dropped: 0,
    });
  });
});

describe('percent', () => {
  it('rounds to a whole percentage', () => {
    expect(percent(0.7)).toBe(70);
    expect(percent(0.95)).toBe(95);
    expect(percent(0.005)).toBe(1);
  });
});
