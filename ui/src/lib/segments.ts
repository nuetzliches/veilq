import type { Finding, Location } from '@/api';

/** A piece of a text: plain, or the span of finding `finding`. */
export interface Segment {
  text: string;
  finding?: number;
}

/**
 * `text` cut at `spans`: byte ranges into its UTF-8 form (as veilq reports
 * them), one per finding, `null` for a finding without a span here. Spans
 * that overlap an earlier one or lie outside the text are left out.
 */
export function segments(text: string, spans: ([number, number] | null)[]): Segment[] {
  const data = new TextEncoder().encode(text);
  const decoder = new TextDecoder();
  const order = spans
    .map((s, i) => ({ s, i }))
    .filter((x): x is { s: [number, number]; i: number } => x.s !== null && x.s[0] <= x.s[1] && x.s[1] <= data.length)
    .sort((a, b) => a.s[0] - b.s[0] || a.s[1] - b.s[1]);
  const out: Segment[] = [];
  let at = 0;
  for (const { s, i } of order) {
    if (s[0] < at) continue;
    if (s[0] > at) out.push({ text: decoder.decode(data.subarray(at, s[0])) });
    out.push({ text: decoder.decode(data.subarray(s[0], s[1])), finding: i });
    at = s[1];
  }
  if (at < data.length || out.length === 0) out.push({ text: decoder.decode(data.subarray(at)) });
  return out;
}

/** The input spans of `findings`: a plain text's spans and cells. */
export function inputSpans(findings: Finding[]): ([number, number] | null)[] {
  return findings.map((f) => (f.location.kind === 'span' && !f.location.part ? [f.location.start, f.location.end] : null));
}

export function describeLocation(location: Location): string {
  switch (location.kind) {
    case 'span':
      return `${location.part ? `${location.part}, ` : ''}line ${location.line}, col ${location.column}`;
    case 'cell':
      return `${location.sheet ? `${location.sheet}, ` : ''}row ${location.row}, col ${location.column}`;
    case 'region':
      return `${location.frame ? `frame ${location.frame}, ` : ''}${location.width}×${location.height} at ${location.x},${location.y}`;
  }
}

export function formatSize(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KiB`;
  return `${(size / 1024 / 1024).toFixed(1)} MiB`;
}
