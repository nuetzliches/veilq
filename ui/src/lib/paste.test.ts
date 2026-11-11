import { describe, expect, it } from 'vitest';
import { pastedName } from './paste';

describe('pastedName', () => {
  it('names a screenshot by its type', () => {
    expect(pastedName('image.png', 'image/png')).toBe('clipboard.png');
    expect(pastedName('', 'image/jpeg')).toBe('clipboard.jpg');
    expect(pastedName('', 'application/x-unknown')).toBe('clipboard.bin');
  });

  it('keeps the name of a copied file', () => {
    expect(pastedName('Brief an Mira.docx', '')).toBe('Brief an Mira.docx');
    expect(pastedName('scan.PDF', 'application/pdf')).toBe('scan.PDF');
  });
});
