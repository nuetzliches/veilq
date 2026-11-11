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
}>();
const emit = defineEmits<{ hover: [index: number | null]; open: []; select: [index: number]; sized: [] }>();

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
</script>

<template>
  <div class="pane-image" :class="[side, { fit: zoom === 'fit', scaled: typeof zoom === 'number' }]">
    <p v-if="page" class="page">{{ t('pdf.page', { page }) }}</p>
    <div class="frame" :data-anchor="anchor" @dblclick="emit('open')">
      <img :src="src" alt="" :style="imgStyle" draggable="false" @load="onLoad" />
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
