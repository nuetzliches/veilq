<script setup lang="ts">
import { computed, ref } from 'vue';
import type { Finding } from '@/api';

// An image beside its redacted twin: each finding of the image itself (a
// region without a part) is a frame over it, on both sides; a PDF's page
// shows the findings of its `page`.
const props = defineProps<{
  data: string;
  mediaType: string;
  findings: Finding[];
  hovered: number | null;
  side: 'input' | 'output';
  page?: number;
}>();
const emit = defineEmits<{ hover: [index: number | null] }>();

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

function onLoad(e: Event) {
  const img = e.target as HTMLImageElement;
  size.value = { w: img.naturalWidth, h: img.naturalHeight };
}
</script>

<template>
  <div class="pane-image" :class="side">
    <p v-if="page" class="page">Page {{ page }}</p>
    <div class="frame">
      <img :src="src" alt="" @load="onLoad" />
      <span
        v-for="b in boxes"
        :key="b.i"
        class="box"
        :class="{ hot: hovered === b.i }"
        :style="b.style"
        :title="`${b.entity} · ${b.detector}`"
        @mouseenter="emit('hover', b.i)"
        @mouseleave="emit('hover', null)"
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
.box {
  position: absolute;
  border: 2px solid var(--danger-text);
  border-radius: 2px;
  cursor: default;
  transition: box-shadow 0.12s var(--ease);
}
.output .box {
  border-color: var(--ok-text);
}
.box.hot {
  box-shadow: 0 0 0 3px var(--accent);
}
</style>
