import { describe, expect, it } from 'vitest';
import { copyablePicture, copyIsOurs } from './clipboard';

describe('clipboard', () => {
  it('offers still pictures, not a GIF or a PDF', () => {
    expect(copyablePicture('image/png')).toBe(true);
    expect(copyablePicture('image/jpeg')).toBe(true);
    expect(copyablePicture('image/gif')).toBe(false);
    expect(copyablePicture('application/pdf')).toBe(false);
  });

  it('leaves a selection and a field to the browser', () => {
    const body = document.createElement('div');
    const field = document.createElement('textarea');
    expect(copyIsOurs(body, '')).toBe(true);
    expect(copyIsOurs(body, 'Mira')).toBe(false);
    expect(copyIsOurs(field, '')).toBe(false);
    expect(copyIsOurs(null, '')).toBe(true);
  });
});
