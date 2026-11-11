<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import type { Finding } from '@/api';
import ImagePane from '@/components/ImagePane.vue';

/** The pictures of one side: an image, or a PDF's pages. */
export interface Pictures {
  pages: string[];
  mediaType: string;
  paged: boolean;
}

// An image (or a PDF's pages) over the whole window, to zoom into: fitted
// to the window, at its natural size or scaled, with the same frames as in
// the panes. Input and output can be switched without losing the zoom.
const props = defineProps<{
  input: Pictures | null;
  output: Pictures | null;
  side: 'input' | 'output';
  findings: Finding[];
  hovered: number | null;
}>();
const emit = defineEmits<{ close: []; hover: [index: number | null] }>();

const MIN = 0.1;
const MAX = 8;
const STEP = 1.25;

const side = ref(props.side);
/** `fit`, or a scale of the natural size. */
const zoom = ref<'fit' | number>('fit');
const root = ref<HTMLElement | null>(null);
const body = ref<HTMLElement | null>(null);
const fullscreen = ref(false);

const shown = computed(() => (side.value === 'input' ? props.input : props.output) ?? props.input ?? props.output);
const label = computed(() => (zoom.value === 'fit' ? 'Fit' : `${Math.round(zoom.value * 100)} %`));

/** The scale the first picture is shown at now: in `fit`, as measured. */
function currentScale(): number {
  if (typeof zoom.value === 'number') return zoom.value;
  const img = body.value?.querySelector('img');
  return img && img.naturalWidth ? img.clientWidth / img.naturalWidth : 1;
}

async function zoomTo(scale: number, anchor?: { x: number; y: number }) {
  const el = body.value;
  const before = currentScale();
  const next = Math.min(MAX, Math.max(MIN, scale));
  // Keep the point under the anchor (the pointer, else the middle) in place.
  const ax = anchor?.x ?? (el ? el.clientWidth / 2 : 0);
  const ay = anchor?.y ?? (el ? el.clientHeight / 2 : 0);
  const px = el ? (el.scrollLeft + ax) / before : 0;
  const py = el ? (el.scrollTop + ay) / before : 0;
  zoom.value = next;
  await nextTick();
  if (el) {
    el.scrollLeft = px * next - ax;
    el.scrollTop = py * next - ay;
  }
}

const zoomIn = () => zoomTo(currentScale() * STEP);
const zoomOut = () => zoomTo(currentScale() / STEP);
const fit = () => (zoom.value = 'fit');
const actual = () => zoomTo(1);

function onWheel(e: WheelEvent) {
  if (!e.ctrlKey && !e.metaKey) return;
  e.preventDefault();
  const r = body.value!.getBoundingClientRect();
  void zoomTo(currentScale() * (e.deltaY < 0 ? STEP : 1 / STEP), { x: e.clientX - r.left, y: e.clientY - r.top });
}

// Drag to pan once the picture is larger than the window.
let drag: { x: number; y: number; left: number; top: number } | null = null;
function onPointerDown(e: PointerEvent) {
  const el = body.value;
  if (!el || e.button !== 0) return;
  drag = { x: e.clientX, y: e.clientY, left: el.scrollLeft, top: el.scrollTop };
  el.setPointerCapture(e.pointerId);
}
function onPointerMove(e: PointerEvent) {
  const el = body.value;
  if (!drag || !el) return;
  el.scrollLeft = drag.left - (e.clientX - drag.x);
  el.scrollTop = drag.top - (e.clientY - drag.y);
}
function onPointerUp() {
  drag = null;
}

async function toggleFullscreen() {
  if (document.fullscreenElement) await document.exitFullscreen();
  else await root.value?.requestFullscreen();
}

function onFullscreenChange() {
  fullscreen.value = document.fullscreenElement === root.value;
}

function close() {
  if (document.fullscreenElement) void document.exitFullscreen();
  emit('close');
}

function onKey(e: KeyboardEvent) {
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  const actions: Record<string, () => unknown> = {
    Escape: close,
    '+': zoomIn,
    '=': zoomIn,
    '-': zoomOut,
    '0': fit,
    '1': actual,
    f: toggleFullscreen,
    i: () => props.input && (side.value = 'input'),
    o: () => props.output && (side.value = 'output'),
  };
  const act = actions[e.key];
  if (act) {
    e.preventDefault();
    void act();
  }
}

watch(
  () => props.side,
  (s) => (side.value = s),
);

onMounted(() => {
  window.addEventListener('keydown', onKey);
  document.addEventListener('fullscreenchange', onFullscreenChange);
  root.value?.focus();
});
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKey);
  document.removeEventListener('fullscreenchange', onFullscreenChange);
});
</script>

<template>
  <div ref="root" class="viewer" role="dialog" aria-modal="true" aria-label="Image viewer" tabindex="-1">
    <header class="viewer-head">
      <div class="sides" role="tablist">
        <button
          role="tab"
          :aria-selected="side === 'input'"
          :class="{ active: side === 'input' }"
          :disabled="!input"
          @click="side = 'input'"
        >
          Input <kbd>I</kbd>
        </button>
        <button
          role="tab"
          :aria-selected="side === 'output'"
          :class="{ active: side === 'output' }"
          :disabled="!output"
          @click="side = 'output'"
        >
          Output <kbd>O</kbd>
        </button>
      </div>
      <span class="spacer" />
      <button class="small" title="Zoom out (−, Ctrl+wheel)" @click="zoomOut">−</button>
      <span class="mono level">{{ label }}</span>
      <button class="small" title="Zoom in (+, Ctrl+wheel)" @click="zoomIn">+</button>
      <button class="small" :class="{ on: zoom === 'fit' }" title="Fit to the window (0)" @click="fit">Fit</button>
      <button class="small" :class="{ on: zoom === 1 }" title="Natural size (1)" @click="actual">1:1</button>
      <button class="small" title="Fullscreen (F)" @click="toggleFullscreen">
        {{ fullscreen ? 'Leave fullscreen' : 'Fullscreen' }}
      </button>
      <button class="small" title="Close (Esc)" @click="close">Close</button>
    </header>
    <div
      ref="body"
      class="viewer-body"
      :class="{ pan: zoom !== 'fit' }"
      @wheel="onWheel"
      @pointerdown="onPointerDown"
      @pointermove="onPointerMove"
      @pointerup="onPointerUp"
      @pointercancel="onPointerUp"
    >
      <template v-if="shown">
        <ImagePane
          v-for="(p, i) in shown.pages"
          :key="`${side}-${i}`"
          :side="side"
          :data="p"
          :media-type="shown.mediaType"
          :page="shown.paged ? i + 1 : undefined"
          :findings="findings"
          :hovered="hovered"
          :zoom="zoom"
          @hover="emit('hover', $event)"
        />
      </template>
    </div>
  </div>
</template>

<style scoped>
.viewer {
  position: fixed;
  inset: 0;
  z-index: 10;
  display: flex;
  flex-direction: column;
  background: var(--bg);
}
.viewer:focus {
  outline: none;
}
.viewer-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  height: 48px;
  padding: 0 var(--space-4);
  border-bottom: 1px solid var(--border);
  background: var(--bg-elev);
}
.sides {
  display: flex;
  gap: 2px;
}
.sides button {
  border: 0;
  background: none;
  padding: 4px var(--space-3);
  font-size: 12.5px;
  color: var(--text-2);
}
.sides button.active {
  background: var(--accent-soft);
  color: var(--accent-text);
  font-weight: 500;
}
.spacer {
  flex: 1;
}
.level {
  min-width: 52px;
  text-align: center;
  color: var(--text-muted);
}
button.on {
  background: var(--accent-soft);
  color: var(--accent-text);
}
.viewer-body {
  flex: 1;
  overflow: auto;
  text-align: center;
}
.viewer-body.pan {
  cursor: grab;
  text-align: left;
}
.viewer-body.pan:active {
  cursor: grabbing;
}
kbd {
  font-size: 11px;
  opacity: 0.6;
  margin-left: 2px;
}
</style>
