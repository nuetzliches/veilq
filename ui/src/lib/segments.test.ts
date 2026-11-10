import { describe, expect, it } from 'vitest';
import { mockRedact } from '@/api/mock';
import { takeToken } from '@/api/client';
import { describeLocation, segments } from './segments';

describe('segments', () => {
  it('cuts at byte offsets, not UTF-16 offsets', () => {
    // "Grüße " is 8 bytes in UTF-8, 6 code units.
    const text = 'Grüße Mira Albers.';
    const s = segments(text, [[8, 19]]);
    expect(s).toEqual([{ text: 'Grüße ' }, { text: 'Mira Albers', finding: 0 }, { text: '.' }]);
  });

  it('skips overlaps, spans out of range and findings without a span', () => {
    const s = segments('abcdef', [[1, 4], [2, 5], null, [3, 99]]);
    expect(s).toEqual([{ text: 'a' }, { text: 'bcd', finding: 0 }, { text: 'ef' }]);
  });

  it('keeps an empty text as one segment', () => {
    expect(segments('', [])).toEqual([{ text: '' }]);
  });

  it('marks input and output of a redaction at the same findings', () => {
    const r = mockRedact('Mail an lena.vogt@example.org, Grüße Jonas Brandt');
    const input = segments(r.input, r.input_spans).filter((x) => x.finding !== undefined);
    const output = segments(r.output, r.output_spans).filter((x) => x.finding !== undefined);
    expect(input.map((x) => x.text)).toEqual(['lena.vogt@example.org', 'Jonas Brandt']);
    expect(output.map((x) => x.text)).toEqual(['<EMAIL_ADDRESS>', '<PERSON>']);
    expect(r.output).toBe('Mail an <EMAIL_ADDRESS>, Grüße <PERSON>');
  });
});

describe('takeToken', () => {
  it('moves the token from the fragment into session storage', () => {
    const storage = new Map<string, string>();
    const fakeStorage = {
      getItem: (k: string) => storage.get(k) ?? null,
      setItem: (k: string, v: string) => void storage.set(k, v),
    } as unknown as Storage;
    let replaced = '';
    const fakeHistory = { replaceState: (_: unknown, __: string, url: string) => (replaced = url) } as unknown as History;
    const location = { hash: '#token=ab12', pathname: '/', search: '' } as Location;
    expect(takeToken(location, fakeStorage, fakeHistory)).toBe('ab12');
    expect(replaced).toBe('/');
    const later = { hash: '', pathname: '/', search: '' } as Location;
    expect(takeToken(later, fakeStorage, fakeHistory)).toBe('ab12');
  });
});

describe('describeLocation', () => {
  it('names the part and page of a region', () => {
    expect(describeLocation({ kind: 'region', x: 4, y: 8, width: 20, height: 10 })).toBe('20×10 at 4,8');
    expect(
      describeLocation({ kind: 'region', part: 'word/media/image1.png', frame: 2, x: 4, y: 8, width: 20, height: 10 }),
    ).toBe('word/media/image1.png, page/frame 2, 20×10 at 4,8');
  });
});
