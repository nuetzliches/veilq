<script setup lang="ts">
import { computed } from 'vue';
import type { Finding } from '@/api';
import { segments } from '@/lib/segments';

const props = defineProps<{
  text: string;
  spans: ([number, number] | null)[];
  findings: Finding[];
  hovered: number | null;
  side: 'input' | 'output';
}>();
const emit = defineEmits<{ hover: [index: number | null] }>();

const parts = computed(() => segments(props.text, props.spans));
</script>

<template>
  <pre class="pane-text" :class="side"><template v-for="(p, i) in parts" :key="i"><mark
        v-if="p.finding !== undefined"
        :class="{ hot: hovered === p.finding }"
        :data-entity="findings[p.finding]?.entity"
        :title="`${findings[p.finding]?.entity} · ${findings[p.finding]?.detector}`"
        @mouseenter="emit('hover', p.finding)"
        @mouseleave="emit('hover', null)"
      >{{ p.text }}</mark><template v-else>{{ p.text }}</template></template></pre>
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
  cursor: default;
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
</style>
