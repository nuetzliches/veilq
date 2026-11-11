import { describe, expect, it } from 'vitest';
import type { Finding, Redaction } from '@/api';
import { mockRedact } from '@/api/mock';
import { type Anchor, clampZoom, locate, place, shownFindings, textAnchor } from './view';

const box = (key: string, top: number, height: number, left = 0, width = 100): Anchor => ({
  key,
  left,
  top,
  width,
  height,
});

describe('locate and place', () => {
  it('maps a point to the same anchor on a side where it is taller', () => {
    // "Mira Albers" on the left, "[PERSON_1]" wrapping onto two lines on the right.
    const input = [box('p-1', 0, 40), box('f0', 40, 20), box('p0', 40, 200)];
    const output = [box('p-1', 0, 40), box('f0', 40, 40), box('p0', 60, 200)];
    const point = locate(input, 50, 140)!;
    expect(point).toEqual({ key: 'p0', fx: 0.5, fy: 0.5 });
    expect(place(output, point)).toEqual({ x: 50, y: 160 });
  });

  it('takes the later of two anchors that start on the same line', () => {
    const anchors = [box('p-1', 0, 20), box('f0', 0, 20, 60, 30)];
    expect(locate(anchors, 70, 10)?.key).toBe('f0');
  });

  it('keeps a point in the gap below an anchor at its end', () => {
    const pages = [box('pg0', 16, 100), box('pg1', 148, 100)];
    expect(locate(pages, 50, 130)).toEqual({ key: 'pg0', fx: 0.5, fy: 1 });
  });

  it('puts a point above every anchor into the first', () => {
    expect(locate([box('pg0', 16, 100)], 0, 4)).toEqual({ key: 'pg0', fx: 0, fy: 0 });
    expect(locate([], 0, 0)).toBeNull();
  });

  it('places nothing where the other side lacks the anchor', () => {
    expect(place([box('p-1', 0, 10)], { key: 'f3', fx: 0, fy: 0 })).toBeNull();
  });

  it('names text anchors by the finding before them', () => {
    expect(textAnchor(undefined, -1)).toBe('p-1');
    expect(textAnchor(2, 1)).toBe('f2');
  });

  it('keeps the zoom between its limits', () => {
    expect(clampZoom(100)).toBe(8);
    expect(clampZoom(0)).toBe(0.1);
    expect(clampZoom(0.2, true)).toBe(0.5);
    expect(clampZoom(5, true)).toBe(4);
  });
});

describe('shownFindings', () => {
  const region = (frame?: number, part?: string): Finding => ({
    entity: 'PERSON',
    confidence: 0.9,
    detector: 'ocr',
    location: { kind: 'region', x: 0, y: 0, width: 1, height: 1, frame, part },
  });
  const picture = (findings: Finding[], extra: Partial<Redaction>): Redaction => ({
    input: '',
    output: '',
    input_spans: [],
    output_spans: [],
    report: { version: 1, findings, review_required: false },
    ...extra,
  });

  it('shows a text finding with a span', () => {
    const r = mockRedact('Mira Albers, mira.albers@example.com');
    expect(shownFindings(r).every(Boolean)).toBe(true);
  });

  it('shows the regions of a still image, not those inside a part', () => {
    const r = picture([region(), region(undefined, 'word/media/image1.png')], {
      media_type: 'image/png',
      input_base64: 'AA==',
      output_base64: 'AA==',
    });
    expect(shownFindings(r)).toEqual([true, false]);
  });

  it('shows no frame of a GIF', () => {
    const r = picture([region(), region(2)], { media_type: 'image/gif', input_base64: 'AA==', output_base64: 'AA==' });
    expect(shownFindings(r)).toEqual([false, false]);
  });

  it('shows a PDF region on a rendered page only', () => {
    const r = picture([region(1), region(3), region()], {
      media_type: 'application/pdf',
      output_base64: 'AA==',
      input_pages: ['AA==', 'AA=='],
      output_pages: ['AA==', 'AA=='],
      page_count: 5,
    });
    expect(shownFindings(r)).toEqual([true, false, false]);
  });

  it('shows nothing of an Office file', () => {
    const r = picture([region(undefined, 'word/document.xml')], {
      media_type: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
      output_base64: 'AA==',
    });
    expect(shownFindings(r)).toEqual([false]);
  });
});
