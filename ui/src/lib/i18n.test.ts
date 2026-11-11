import { describe, expect, it } from 'vitest';
import { de } from '@/lib/messages/de';
import { en } from '@/lib/messages/en';
import { format, systemLocale } from './i18n';
import { describeLocation, missingModels } from './labels';

describe('systemLocale', () => {
  it('takes German where the first language is German, English otherwise', () => {
    expect(systemLocale(['de-DE', 'en-US'])).toBe('de');
    expect(systemLocale(['de-AT'])).toBe('de');
    expect(systemLocale(['en-GB', 'de-DE'])).toBe('en');
    expect(systemLocale(['fr-FR'])).toBe('en');
    expect(systemLocale([])).toBe('en');
  });
});

describe('format', () => {
  it('fills named placeholders and leaves unknown braces alone', () => {
    expect(format('{n} findings', { n: 3 })).toBe('3 findings');
    expect(format('regex, e.g. KD-\\d{6}')).toBe('regex, e.g. KD-\\d{6}');
  });
});

describe('catalogues', () => {
  const placeholders = (s: string) => [...s.matchAll(/\{([a-z]\w*)\}/g)].map((m) => m[1]).sort();

  it('have the same keys', () => {
    expect(Object.keys(de).sort()).toEqual(Object.keys(en).sort());
  });

  it('use the same placeholders per message', () => {
    for (const [key, message] of Object.entries(en)) {
      expect(placeholders(de[key as keyof typeof en]), key).toEqual(placeholders(message));
    }
  });

  it('write German with umlauts, not their ASCII spelling', () => {
    for (const [key, message] of Object.entries(de)) {
      expect(message, key).not.toMatch(/schwaerz|fuer\b|ueber\b|oeffn|schliess|groess|waehl|loesch|pruef|aender/i);
    }
  });
});

describe('labels', () => {
  it('finds the models a message of veilq asks for', () => {
    expect(missingModels('images need OCR, which is not installed: `veilq models fetch ocr`')).toEqual(['ocr']);
    expect(
      missingModels('PDFs need OCR and PDFium: `veilq models fetch ocr` and `veilq models fetch pdf`\n`veilq models fetch ocr`'),
    ).toEqual(['ocr', 'pdf']);
    expect(missingModels('nothing missing')).toEqual([]);
  });

  it('says where a finding is', () => {
    expect(describeLocation({ kind: 'span', start: 0, end: 4, line: 3, column: 7 })).toBe('line 3, column 7');
    expect(describeLocation({ kind: 'region', part: 'word/media/image1.png', frame: 2, x: 4, y: 8, width: 20, height: 10 })).toBe(
      'word/media/image1.png, page 2',
    );
  });
});
