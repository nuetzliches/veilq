import { describe, expect, it } from 'vitest';
import { AREA_TYPES, byteSlice, MARK_TYPES, markable } from './marks';

describe('marks', () => {
  it('offers a face and a signature for an area only, every text type too', () => {
    expect(AREA_TYPES).toContain('FACE');
    expect(AREA_TYPES).toContain('SIGNATURE');
    expect(MARK_TYPES).not.toContain('FACE' as never);
    for (const e of MARK_TYPES) expect(AREA_TYPES).toContain(e);
    expect(new Set(AREA_TYPES).size).toBe(AREA_TYPES.length);
  });

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
