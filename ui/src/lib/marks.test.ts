import { describe, expect, it } from 'vitest';
import { byteSlice, markable } from './marks';

describe('marks', () => {
  it('slices by bytes, as the API counts', () => {
    const text = 'Grüße an Lena Brandt';
    // `ü` and `ß` are two bytes each: the name starts at byte 11.
    expect(byteSlice(text, [11, 22])).toBe('Lena Brandt');
  });

  it('takes a short value on one line, never a placeholder', () => {
    expect(markable('  Falkenstein ')).toBe('Falkenstein');
    expect(markable('')).toBeNull();
    expect(markable('a\nb')).toBeNull();
    expect(markable('x'.repeat(201))).toBeNull();
    expect(markable('Hallo <PERSON>')).toBeNull();
    expect(markable('[PERSON:0a1b2c3d]')).toBeNull();
  });
});
