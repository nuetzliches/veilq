import type { Finding, Redaction } from '@/api';
import { segments } from '@/lib/segments';

// One view over both sides (ADR-0036): the input and the output share a zoom
// and the place they show. A place is not a scroll offset, which differs
// between the sides once a placeholder is longer than the name it replaced,
// but a point in an anchor both sides render: a page of a picture, or a
// piece of a text (a finding, or the plain text after one). The text between
// two findings is the same on both sides, so the same anchor shows the same
// words.

export const ZOOM_STEP = 1.25;

/** `fit` (a picture fits the pane, a text has its own size), or a scale. */
export type Zoom = 'fit' | number;

/** A scale between the limits: of a picture's natural size, or of the text's size. */
export function clampZoom(scale: number, text = false): number {
  const [min, max] = text ? [0.5, 4] : [0.1, 8];
  return Math.min(max, Math.max(min, scale));
}

/** A point of the content: in anchor `key`, at a fraction of its width and height. */
export interface ViewPoint {
  key: string;
  fx: number;
  fy: number;
}

/** An anchor's box in the content of its scroll container. */
export interface Anchor {
  key: string;
  left: number;
  top: number;
  width: number;
  height: number;
}

/** The anchor key of a piece of text: the finding, or the plain text after finding `after` (-1: the start). */
export function textAnchor(finding: number | undefined, after: number): string {
  return finding !== undefined ? `f${finding}` : `p${after}`;
}

/** The anchor key of page `index` of a picture (0 for a single image). */
export function pageAnchor(index: number): string {
  return `pg${index}`;
}

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

/**
 * The point at content position (`x`, `y`) among `count` anchors in
 * document order, which is top to bottom: in the last anchor that starts at
 * or above `y` (of two on one line, the later), so a point in the gap below
 * an anchor stays at its end. A binary search: a long text has tens of
 * thousands of anchors, and a scroll measures only a few of them.
 */
function locateAmong(count: number, at: (i: number) => Anchor, x: number, y: number): ViewPoint | null {
  if (!count) return null;
  let lo = 0;
  let hi = count - 1;
  while (lo < hi) {
    const mid = Math.ceil((lo + hi) / 2);
    if (at(mid).top <= y) lo = mid;
    else hi = mid - 1;
  }
  const best = at(lo);
  return {
    key: best.key,
    fx: best.width ? clamp01((x - best.left) / best.width) : 0,
    fy: best.height ? clamp01((y - best.top) / best.height) : 0,
  };
}

/** The point at content position (`x`, `y`) among `anchors`, in document order. */
export function locate(anchors: Anchor[], x: number, y: number): ViewPoint | null {
  return locateAmong(anchors.length, (i) => anchors[i], x, y);
}

/** The content position of `point` among `anchors`, or null when no anchor has its key. */
export function place(anchors: Anchor[], point: ViewPoint): { x: number; y: number } | null {
  const a = anchors.find((b) => b.key === point.key);
  if (!a) return null;
  return { x: a.left + point.fx * a.width, y: a.top + point.fy * a.height };
}

/** Anchor `el` under scroll container `body`, in its content's coordinates. */
function measure(body: HTMLElement, el: HTMLElement): Anchor {
  const r = body.getBoundingClientRect();
  const b = el.getBoundingClientRect();
  return {
    key: el.dataset.anchor!,
    left: b.left - r.left + body.scrollLeft,
    top: b.top - r.top + body.scrollTop,
    width: b.width,
    height: b.height,
  };
}

/** The point `body` shows at (`x`, `y`) of its visible area, the middle by default. */
export function pointIn(body: HTMLElement, x = body.clientWidth / 2, y = body.clientHeight / 2): ViewPoint | null {
  const els = body.querySelectorAll<HTMLElement>('[data-anchor]');
  return locateAmong(els.length, (i) => measure(body, els[i]), body.scrollLeft + x, body.scrollTop + y);
}

/** Where `body` scrolls to show `point` at (`x`, `y`) of its visible area; null without its anchor. */
export function scrollFor(
  body: HTMLElement,
  point: ViewPoint,
  x = body.clientWidth / 2,
  y = body.clientHeight / 2,
): { left: number; top: number } | null {
  const el = body.querySelector<HTMLElement>(`[data-anchor="${point.key}"]`);
  const p = el && place([measure(body, el)], point);
  return p && { left: p.x - x, top: p.y - y };
}

/** The middle of finding `index` (`data-finding`) in `body`'s content, or null when it shows none. */
export function findingIn(body: HTMLElement, index: number): { x: number; y: number } | null {
  const el = body.querySelector<HTMLElement>(`[data-finding="${index}"]`);
  if (!el) return null;
  const r = body.getBoundingClientRect();
  const b = el.getBoundingClientRect();
  return { x: b.left - r.left + body.scrollLeft + b.width / 2, y: b.top - r.top + body.scrollTop + b.height / 2 };
}

/**
 * Two scroll containers that show the same point: a scroll of one, by the
 * user, moves the other. A scroll set here is not followed back.
 */
export function linkPanes(panes: () => (HTMLElement | null)[]) {
  const set = new WeakMap<HTMLElement, { left: number; top: number }>();
  let frame = 0;

  /** Scrolls `el`, without following the move to the other pane. */
  function scrollTo(el: HTMLElement, left: number, top: number) {
    const before = { left: el.scrollLeft, top: el.scrollTop };
    el.scrollLeft = left;
    el.scrollTop = top;
    if (el.scrollLeft !== before.left || el.scrollTop !== before.top) {
      set.set(el, { left: el.scrollLeft, top: el.scrollTop });
    }
  }

  /**
   * Moves the other pane to the point `from` shows in its middle, at the
   * same height: a pane with a footer is lower, not offset.
   */
  function follow(from: HTMLElement) {
    const x = from.clientWidth / 2;
    const y = from.clientHeight / 2;
    const point = pointIn(from, x, y);
    if (!point) return;
    for (const to of panes()) {
      if (!to || to === from) continue;
      const s = scrollFor(to, point, x, y);
      if (s) scrollTo(to, s.left, s.top);
    }
  }

  function onScroll(event: Event) {
    const el = event.currentTarget as HTMLElement;
    const s = set.get(el);
    if (s) {
      set.delete(el);
      if (Math.abs(s.left - el.scrollLeft) < 2 && Math.abs(s.top - el.scrollTop) < 2) return;
    }
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => follow(el));
  }

  return { scrollTo, follow, onScroll };
}

/**
 * Drag to pan a scroll container. A drag starts after a few pixels, so a
 * click and a double click on what it shows still arrive.
 */
export function dragPan(enabled: () => boolean) {
  let drag: { x: number; y: number; left: number; top: number; id: number; moving: boolean } | null = null;
  return {
    down(e: PointerEvent) {
      const el = e.currentTarget as HTMLElement;
      if (!enabled() || e.button !== 0) return;
      drag = { x: e.clientX, y: e.clientY, left: el.scrollLeft, top: el.scrollTop, id: e.pointerId, moving: false };
    },
    move(e: PointerEvent) {
      const el = e.currentTarget as HTMLElement;
      if (!drag || e.pointerId !== drag.id) return;
      const dx = e.clientX - drag.x;
      const dy = e.clientY - drag.y;
      if (!drag.moving) {
        if (Math.hypot(dx, dy) < 4) return;
        drag.moving = true;
        el.setPointerCapture(e.pointerId);
      }
      el.scrollLeft = drag.left - dx;
      el.scrollTop = drag.top - dy;
    },
    up() {
      drag = null;
    },
  };
}

/**
 * Which findings the panes can show and a click can go to: a text's
 * findings with a span on either side, a still image's regions, a rendered
 * PDF page's regions. A region in a part (a picture inside a document), a
 * page past the rendered ones and a GIF's frames have no place in the view.
 */
export function shownFindings(result: Redaction | null): boolean[] {
  if (!result) return [];
  const findings = result.report.findings;
  const pictured = (f: Finding, pages: number | null) => {
    const l = f.location;
    if (l.kind !== 'region' || l.part !== undefined) return false;
    return pages === null ? l.frame === undefined : l.frame !== undefined && l.frame >= 1 && l.frame <= pages;
  };
  if (result.input_pages && result.output_pages) {
    const pages = Math.min(result.input_pages.length, result.output_pages.length);
    return findings.map((f) => pictured(f, pages));
  }
  if (result.media_type?.startsWith('image/') && result.input_base64 && result.output_base64) {
    if (result.media_type === 'image/gif') return findings.map(() => false);
    return findings.map((f) => pictured(f, null));
  }
  if (result.output_base64) return findings.map(() => false);
  const seen = new Set<number>();
  for (const [text, spans] of [
    [result.input, result.input_spans],
    [result.output, result.output_spans],
  ] as const) {
    for (const s of segments(text, spans)) if (s.finding !== undefined) seen.add(s.finding);
  }
  return findings.map((_, i) => seen.has(i));
}
