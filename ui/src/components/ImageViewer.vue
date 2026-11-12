<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import type { Finding } from '@/api';
import ImagePane from '@/components/ImagePane.vue';
import { t } from '@/lib/i18n';
import {
  clampZoom,
  dragPan,
  findingIn,
  pageAnchor,
  pointIn,
  scrollFor,
  type ViewPoint,
  type Zoom,
  ZOOM_STEP,
} from '@/lib/view';

/** The pictures of one side: an image, or a PDF's pages. */
export interface Pictures {
  pages: string[];
  mediaType: string;
  paged: boolean;
}

// An image (or a PDF's pages) over the whole window, to zoom into: fitted
// to the window, at its natural size or scaled, with the same frames as in
// the panes. Input and output can be switched without losing the zoom. The
// zoom is the panes' (ADR-0036): the viewer opens on the place the panes
// show, and gives back the place it shows when it closes.
const props = defineProps<{
  input: Pictures | null;
  output: Pictures | null;
  side: 'input' | 'output';
  findings: Finding[];
  hovered: number | null;
  selected: number | null;
  /** The place to open on: the middle of the pane it was opened from. */
  at: ViewPoint | null;
}>();
const emit = defineEmits<{ close: [at: ViewPoint | null]; hover: [index: number | null]; select: [index: number] }>();
const zoom = defineModel<Zoom>('zoom', { required: true });

const side = ref(props.side);
const root = ref<HTMLElement | null>(null);
const body = ref<HTMLElement | null>(null);
const fullscreen = ref(false);

const shown = computed(() => (side.value === 'input' ? props.input : props.output) ?? props.input ?? props.output);
const label = computed(() => (zoom.value === 'fit' ? t('viewer.fit') : `${Math.round(zoom.value * 100)} %`));

/** The scale the first picture is shown at now: in `fit`, as measured. */
function currentScale(): number {
  if (typeof zoom.value === 'number') return zoom.value;
  const img = body.value?.querySelector('img');
  return img && img.naturalWidth ? img.clientWidth / img.naturalWidth : 1;
}

/** Shows `point` at (`x`, `y`) of the window, the middle by default. */
function show(point: ViewPoint, x?: number, y?: number) {
  const el = body.value;
  const s = el && scrollFor(el, point, x, y);
  if (el && s) {
    el.scrollLeft = s.left;
    el.scrollTop = s.top;
  }
}

async function zoomTo(next: Zoom, anchor?: { x: number; y: number }) {
  const el = body.value;
  // Keep the point under the anchor (the pointer, else the middle) in place.
  const point = el && pointIn(el, anchor?.x, anchor?.y);
  zoom.value = next === 'fit' ? next : clampZoom(next);
  await nextTick();
  if (point) show(point, anchor?.x, anchor?.y);
}

const zoomIn = () => zoomTo(currentScale() * ZOOM_STEP);
const zoomOut = () => zoomTo(currentScale() / ZOOM_STEP);
const fit = () => zoomTo('fit');
const actual = () => zoomTo(1);

function onWheel(e: WheelEvent) {
  if (!e.ctrlKey && !e.metaKey) return;
  e.preventDefault();
  const r = body.value!.getBoundingClientRect();
  void zoomTo(currentScale() * (e.deltaY < 0 ? ZOOM_STEP : 1 / ZOOM_STEP), { x: e.clientX - r.left, y: e.clientY - r.top });
}

// Drag to pan once the picture is larger than the window.
const pan = dragPan(() => zoom.value !== 'fit');

/** Brings finding `index` into the middle of the window. */
function reveal(index: number) {
  const el = body.value;
  const p = el && findingIn(el, index);
  if (el && p) {
    el.scrollLeft = p.x - el.clientWidth / 2;
    el.scrollTop = p.y - el.clientHeight / 2;
  }
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
  emit('close', body.value && pointIn(body.value));
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
watch(
  () => props.selected,
  (i) => i !== null && reveal(i),
);

// The place to open on needs the pictures at their size, set once each has loaded.
let unsized = (shown.value?.pages.length ?? 0) || 1;
async function onSized() {
  if (--unsized !== 0) return;
  await nextTick();
  if (props.at) show(props.at);
  else if (props.selected !== null) reveal(props.selected);
}

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
          {{ t('input.title') }} <kbd>I</kbd>
        </button>
        <button
          role="tab"
          :aria-selected="side === 'output'"
          :class="{ active: side === 'output' }"
          :disabled="!output"
          @click="side = 'output'"
        >
          {{ t('output.title') }} <kbd>O</kbd>
        </button>
      </div>
      <span class="spacer" />
      <button class="small" :title="t('viewer.zoomOut')" @click="zoomOut">−</button>
      <span class="mono level">{{ label }}</span>
      <button class="small" :title="t('viewer.zoomIn')" @click="zoomIn">+</button>
      <button class="small" :class="{ on: zoom === 'fit' }" :title="t('viewer.fitTitle')" @click="fit">{{ t('viewer.fit') }}</button>
      <button class="small" :class="{ on: zoom === 1 }" :title="t('viewer.actual')" @click="actual">1:1</button>
      <button class="small" :title="t('viewer.fullscreenTitle')" @click="toggleFullscreen">
        {{ fullscreen ? t('viewer.leaveFullscreen') : t('viewer.fullscreen') }}
      </button>
      <button class="small" :title="t('viewer.closeTitle')" @click="close">{{ t('common.close') }}</button>
    </header>
    <div
      ref="body"
      class="viewer-body"
      :class="{ pan: zoom !== 'fit' }"
      @wheel="onWheel"
      @pointerdown="pan.down"
      @pointermove="pan.move"
      @pointerup="pan.up"
      @pointercancel="pan.up"
    >
      <template v-if="shown">
        <ImagePane
          v-for="(p, i) in shown.pages"
          :key="`${side}-${i}`"
          :anchor="pageAnchor(i)"
          :side="side"
          :data="p"
          :media-type="shown.mediaType"
          :page="shown.paged ? i + 1 : undefined"
          :findings="findings"
          :hovered="hovered"
          :selected="selected"
          :zoom="zoom"
          @hover="emit('hover', $event)"
          @select="emit('select', $event)"
          @sized="onSized"
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
