<script setup lang="ts">
import { computed, ref } from 'vue';
import type { Finding } from '@/api';
import { t } from '@/lib/i18n';
import { entityLabel } from '@/lib/labels';

// An image beside its redacted twin: each finding of the image itself (a
// region without a part) is a frame over it, on both sides; a PDF's page
// shows the findings of its `page`. `zoom` is for the viewer: `fit` keeps
// the image inside the window, a number scales its natural size; without
// it the image fits the pane's width. A click on a frame selects its finding.
// `anchor` names the picture for the other side (ADR-0036): the picture,
// not the pane around it, so a point on it is the same at any width.
const props = defineProps<{
  data: string;
  mediaType: string;
  findings: Finding[];
  hovered: number | null;
  selected?: number | null;
  anchor?: string;
  side: 'input' | 'output';
  page?: number;
  zoom?: 'fit' | number;
  /** A drag draws an area to mark (#34), in the picture's pixels. */
  drawing?: boolean;
  /** The area drawn on this picture, while it is offered as a mark. */
  drawn?: { x: number; y: number; width: number; height: number } | null;
}>();
const emit = defineEmits<{
  hover: [index: number | null];
  open: [];
  select: [index: number];
  sized: [];
  draw: [area: { x: number; y: number; width: number; height: number }];
}>();

const size = ref<{ w: number; h: number } | null>(null);
const src = computed(() => `data:${props.mediaType};base64,${props.data}`);

/** Frames only for a still image: a GIF's findings belong to its frames. */
const boxes = computed(() => {
  const s = size.value;
  if (!s || props.mediaType === 'image/gif') return [];
  return props.findings.flatMap((f, i) => {
    const l = f.location;
    if (l.kind !== 'region' || l.part !== undefined || l.frame !== props.page) return [];
    return [
      {
        i,
        entity: f.entity,
        detector: f.detector,
        style: {
          left: `${(l.x / s.w) * 100}%`,
          top: `${(l.y / s.h) * 100}%`,
          width: `${(l.width / s.w) * 100}%`,
          height: `${(l.height / s.h) * 100}%`,
        },
      },
    ];
  });
});

const imgStyle = computed(() =>
  typeof props.zoom === 'number' && size.value ? { width: `${size.value.w * props.zoom}px` } : undefined,
);

function onLoad(e: Event) {
  const img = e.target as HTMLImageElement;
  size.value = { w: img.naturalWidth, h: img.naturalHeight };
  emit('sized');
}

// Drawing an area (#34): from where the pointer went down to where it is, in
// the picture's pixels whatever its size on screen.
const frame = ref<HTMLElement | null>(null);
const start = ref<{ x: number; y: number } | null>(null);
const current = ref<{ x: number; y: number } | null>(null);

function point(e: PointerEvent): { x: number; y: number } | null {
  const s = size.value;
  const el = frame.value;
  if (!s || !el) return null;
  const r = el.getBoundingClientRect();
  const clamp = (v: number, max: number) => Math.max(0, Math.min(max, v));
  return {
    x: Math.round(clamp(((e.clientX - r.left) / r.width) * s.w, s.w)),
    y: Math.round(clamp(((e.clientY - r.top) / r.height) * s.h, s.h)),
  };
}

function area(a: { x: number; y: number }, b: { x: number; y: number }) {
  return { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), width: Math.abs(a.x - b.x), height: Math.abs(a.y - b.y) };
}

function onDown(e: PointerEvent) {
  if (!props.drawing) return;
  e.stopPropagation();
  e.preventDefault();
  start.value = point(e);
  current.value = start.value;
  (e.target as HTMLElement).setPointerCapture?.(e.pointerId);
}

function onMove(e: PointerEvent) {
  if (!props.drawing || !start.value) return;
  e.stopPropagation();
  current.value = point(e);
}

function onUp(e: PointerEvent) {
  if (!props.drawing || !start.value) return;
  e.stopPropagation();
  const end = point(e) ?? current.value;
  const a = end ? area(start.value, end) : null;
  start.value = null;
  current.value = null;
  // A click is no area: at least a few pixels each way.
  if (a && a.width >= 3 && a.height >= 3) emit('draw', a);
}

/** The area being drawn or offered, as a frame over the picture. */
const sketch = computed(() => {
  const s = size.value;
  const a = start.value && current.value ? area(start.value, current.value) : props.drawn;
  if (!s || !a) return null;
  return {
    left: `${(a.x / s.w) * 100}%`,
    top: `${(a.y / s.h) * 100}%`,
    width: `${(a.width / s.w) * 100}%`,
    height: `${(a.height / s.h) * 100}%`,
  };
});
</script>

<template>
  <div class="pane-image" :class="[side, { fit: zoom === 'fit', scaled: typeof zoom === 'number' }]">
    <p v-if="page" class="page">{{ t('pdf.page', { page }) }}</p>
    <div
      ref="frame"
      class="frame"
      :class="{ drawing }"
      :data-anchor="anchor"
      @dblclick="!drawing && emit('open')"
      @pointerdown="onDown"
      @pointermove="onMove"
      @pointerup="onUp"
    >
      <img :src="src" alt="" :style="imgStyle" draggable="false" @load="onLoad" />
      <span v-if="sketch" class="sketch" :style="sketch" />
      <span
        v-for="b in boxes"
        :key="b.i"
        class="box"
        :class="{ hot: hovered === b.i, sel: selected === b.i }"
        :style="b.style"
        :data-finding="b.i"
        :title="`${b.i + 1}. ${entityLabel(b.entity)}`"
        @mouseenter="emit('hover', b.i)"
        @mouseleave="emit('hover', null)"
        @click.stop="emit('select', b.i)"
      />
    </div>
  </div>
</template>

<style scoped>
.pane-image {
  padding: var(--space-4);
}
.page {
  margin: 0 0 var(--space-2);
  font-size: 12px;
  color: var(--text-muted);
}
.frame {
  position: relative;
  display: inline-block;
  max-width: 100%;
}
img {
  display: block;
  max-width: 100%;
  height: auto;
  border: 1px solid var(--border);
}
.fit img {
  max-height: calc(100vh - 120px);
  width: auto;
}
.scaled .frame,
.scaled img {
  max-width: none;
}
.box {
  position: absolute;
  border: 2px solid var(--danger-text);
  border-radius: 2px;
  cursor: pointer;
  transition: box-shadow 0.12s var(--ease);
}
.output .box {
  border-color: var(--ok-text);
}
.frame.drawing {
  cursor: crosshair;
}
.frame.drawing .box {
  pointer-events: none;
}
/* The area being drawn or offered as a mark (#34). */
.sketch {
  position: absolute;
  border: 2px dashed var(--accent);
  background: color-mix(in srgb, var(--accent) 18%, transparent);
  pointer-events: none;
}
.box.hot {
  box-shadow: 0 0 0 3px var(--accent);
}
.box.sel {
  box-shadow: 0 0 0 3px var(--accent);
  animation: flash 0.9s var(--ease);
}
@keyframes flash {
  0%,
  50% {
    box-shadow: 0 0 0 8px var(--accent);
  }
}
</style>
