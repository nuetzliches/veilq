<script setup lang="ts">
import { computed } from 'vue';
import type { Finding } from '@/api';
import { entityLabel } from '@/lib/labels';
import { segments } from '@/lib/segments';
import { textAnchor } from '@/lib/view';

// A text with its findings marked. Each piece is an anchor (ADR-0036), so
// the other side can show the same words; `scale` sizes the text.
const props = defineProps<{
  text: string;
  spans: ([number, number] | null)[];
  findings: Finding[];
  hovered: number | null;
  selected?: number | null;
  scale?: number;
  side: 'input' | 'output';
}>();
const emit = defineEmits<{ hover: [index: number | null]; select: [index: number] }>();

const parts = computed(() => {
  let after = -1;
  return segments(props.text, props.spans).map((p) => {
    const anchor = textAnchor(p.finding, after);
    if (p.finding !== undefined) after = p.finding;
    return { ...p, anchor };
  });
});
const style = computed(() => (props.scale ? { fontSize: `${12.5 * props.scale}px` } : undefined));
</script>

<template>
  <pre class="pane-text" :class="side" :style="style"><template v-for="(p, i) in parts" :key="i"><mark
        v-if="p.finding !== undefined"
        :class="{ hot: hovered === p.finding, sel: selected === p.finding }"
        :data-anchor="p.anchor"
        :data-finding="p.finding"
        :data-entity="findings[p.finding]?.entity"
        :title="`${p.finding + 1}. ${entityLabel(findings[p.finding]?.entity ?? '')}`"
        @mouseenter="emit('hover', p.finding)"
        @mouseleave="emit('hover', null)"
        @click.stop="emit('select', p.finding)"
      >{{ p.text }}</mark><span v-else :data-anchor="p.anchor">{{ p.text }}</span></template></pre>
</template>

<style scoped>
.pane-text {
  margin: 0;
  padding: var(--space-4);
  font-family: var(--font-mono);
  font-size: 12.5px;
  line-height: 1.6;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  min-height: 100%;
}
mark {
  border-radius: 3px;
  padding: 0 1px;
  cursor: pointer;
  transition: box-shadow 0.12s var(--ease);
}
.input mark {
  background: var(--danger-soft);
  color: var(--danger-text);
}
.output mark {
  background: var(--ok-soft);
  color: var(--ok-text);
}
mark.hot {
  box-shadow: 0 0 0 2px var(--accent);
}
mark.sel {
  box-shadow: 0 0 0 2px var(--accent);
  animation: flash 0.9s var(--ease);
}
@keyframes flash {
  0%,
  50% {
    box-shadow: 0 0 0 6px var(--accent);
  }
}
</style>
