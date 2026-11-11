<script setup lang="ts">
import type { Finding } from '@/api';
import { plural, t } from '@/lib/i18n';
import { describeLocation, entityLabel } from '@/lib/labels';

// The panel beside the output: each finding numbered in the order of the
// report, so a number points at the same place in the input, the output and
// the list. The steps to check a redaction (#18, #34) go here too, below the
// findings, ordered by how far data leaves the machine.
defineProps<{ findings: Finding[]; hovered: number | null }>();
const emit = defineEmits<{ hover: [index: number | null]; close: [] }>();
</script>

<template>
  <aside class="panel" :aria-label="t('findings.title')">
    <header>
      <h2>{{ t('findings.title') }}</h2>
      <span class="count">{{ plural(findings.length, 'output.findingsOne', 'output.findingsOther') }}</span>
      <span class="spacer" />
      <button class="small" :aria-label="t('common.close')" @click="emit('close')">×</button>
    </header>
    <ol v-if="findings.length" class="list">
      <li
        v-for="(f, i) in findings"
        :key="i"
        :class="{ hot: hovered === i }"
        @mouseenter="emit('hover', i)"
        @mouseleave="emit('hover', null)"
      >
        <span class="num">{{ i + 1 }}</span>
        <div class="what">
          <strong>{{ entityLabel(f.entity) }}</strong>
          <span>{{ describeLocation(f.location) }}</span>
        </div>
        <span class="sure" :title="t('findings.confidenceTitle', { detector: f.detector })">
          {{ Math.round(f.confidence * 100) }} %
        </span>
      </li>
    </ol>
    <p v-else class="none">{{ t('output.nothingFound') }}</p>
  </aside>
</template>

<style scoped>
.panel {
  grid-area: panel;
  display: flex;
  flex-direction: column;
  min-height: 0;
  border-left: 1px solid var(--border);
  background: var(--surface);
}
header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  height: 44px;
  flex: none;
  padding: 0 var(--space-3) 0 var(--space-4);
  border-bottom: 1px solid var(--border);
  background: var(--surface-2);
}
h2 {
  font-size: 14px;
}
.count {
  color: var(--text-muted);
  font-size: 12.5px;
}
.spacer {
  flex: 1;
}
.list {
  flex: 1;
  overflow: auto;
  margin: 0;
  padding: var(--space-1) 0;
  list-style: none;
}
li {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-2) var(--space-4);
  border-bottom: 1px solid var(--border);
  font-size: 13px;
}
li.hot {
  background: var(--accent-soft);
}
.num {
  flex: none;
  min-width: 22px;
  height: 22px;
  padding: 0 4px;
  border-radius: 99px;
  background: var(--surface-3);
  color: var(--text-2);
  font-size: 11.5px;
  font-variant-numeric: tabular-nums;
  text-align: center;
  line-height: 22px;
}
.what {
  display: flex;
  flex-direction: column;
  min-width: 0;
  flex: 1;
}
.what span {
  color: var(--text-muted);
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sure {
  color: var(--text-muted);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
}
.none {
  padding: var(--space-4);
  color: var(--text-muted);
  font-size: 13px;
}
@media (max-width: 900px) {
  .panel {
    border-left: 0;
    border-top: 1px solid var(--border);
  }
}
</style>
