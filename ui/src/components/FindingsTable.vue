<script setup lang="ts">
import type { Finding } from '@/api';
import { describeLocation } from '@/lib/segments';

defineProps<{ findings: Finding[]; hovered: number | null }>();
const emit = defineEmits<{ hover: [index: number | null] }>();
</script>

<template>
  <table class="findings">
    <thead>
      <tr>
        <th>Entity</th>
        <th>Where</th>
        <th class="num">Confidence</th>
        <th>Detector</th>
        <th>Pseudonym</th>
      </tr>
    </thead>
    <tbody>
      <tr
        v-for="(f, i) in findings"
        :key="i"
        :class="{ hot: hovered === i }"
        @mouseenter="emit('hover', i)"
        @mouseleave="emit('hover', null)"
      >
        <td><span class="entity">{{ f.entity }}</span></td>
        <td>{{ describeLocation(f.location) }}</td>
        <td class="num">{{ f.confidence.toFixed(2) }}</td>
        <td class="mono">{{ f.detector }}</td>
        <td class="mono">{{ f.pseudonym ?? '' }}</td>
      </tr>
    </tbody>
  </table>
</template>

<style scoped>
.findings {
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}
th,
td {
  text-align: left;
  padding: 6px var(--space-3);
  border-bottom: 1px solid var(--border);
}
th {
  color: var(--text-muted);
  font-weight: 500;
  font-size: 12px;
  position: sticky;
  top: 0;
  background: var(--surface);
}
.num {
  text-align: right;
  font-variant-numeric: tabular-nums;
}
tr.hot td {
  background: var(--accent-soft);
}
.entity {
  font-family: var(--font-mono);
  font-size: 12px;
  background: var(--surface-3);
  padding: 1px 6px;
  border-radius: 4px;
}
</style>
