<script setup lang="ts">
import { nextTick, ref, watch } from 'vue';
import type { Finding, Reading } from '@/api';
import ExplainList from '@/components/ExplainList.vue';
import { plural, t } from '@/lib/i18n';
import { describeLocation, entityLabel } from '@/lib/labels';

// The panel beside the output: each finding numbered in the order of the
// report, so a number points at the same place in the input, the output and
// the list. A click on one shows it on both sides (ADR-0036); one the panes
// cannot show says so. The steps to check a redaction (#18, #34) go here
// too, below the findings, ordered by how far data leaves the machine.
// Below them, on request, what veilq read and why it decided (ADR-0034).
const props = defineProps<{
  findings: Finding[];
  hovered: number | null;
  selected: number | null;
  /** Per finding, whether the panes show it. */
  shown: boolean[];
  readings?: Reading[] | null;
  busy?: boolean;
}>();
const emit = defineEmits<{ hover: [index: number | null]; select: [index: number]; close: []; explain: [] }>();

const list = ref<HTMLElement | null>(null);

// A finding selected in a pane or by a key comes into view here too.
watch(
  () => props.selected,
  async (i) => {
    if (i === null) return;
    await nextTick();
    list.value?.children[i]?.scrollIntoView({ block: 'nearest' });
  },
  { immediate: true },
);
</script>

<template>
  <aside class="panel" :aria-label="t('findings.title')">
    <header>
      <h2>{{ t('findings.title') }}</h2>
      <span class="count">{{ plural(findings.length, 'output.findingsOne', 'output.findingsOther') }}</span>
      <span class="spacer" />
      <button class="small" :aria-label="t('common.close')" @click="emit('close')">×</button>
    </header>
    <div class="scroll">
      <ol v-if="findings.length" ref="list" class="list">
        <li
          v-for="(f, i) in findings"
          :key="i"
          :class="{ hot: hovered === i, sel: selected === i }"
          @mouseenter="emit('hover', i)"
          @mouseleave="emit('hover', null)"
        >
          <button
            class="row"
            :aria-current="selected === i"
            :title="shown[i] ? t('findings.showTitle') : t('findings.notShownTitle')"
            @click="emit('select', i)"
          >
            <span class="num">{{ i + 1 }}</span>
            <span class="what">
              <strong>{{ entityLabel(f.entity) }}</strong>
              <span>{{ describeLocation(f.location) }}</span>
              <em v-if="!shown[i]">{{ t('findings.notShown') }}</em>
            </span>
            <span class="sure" :title="t('findings.confidenceTitle', { detector: f.detector })">
              {{ Math.round(f.confidence * 100) }} %
            </span>
          </button>
        </li>
      </ol>
      <p v-else class="none">{{ t('output.nothingFound') }}</p>
      <ExplainList v-if="readings" :readings="readings" />
      <p v-else class="more">
        <button class="small" :disabled="busy" :title="t('findings.explainTitle')" @click="emit('explain')">
          {{ t('findings.explain') }}
        </button>
      </p>
    </div>
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
.scroll {
  flex: 1;
  overflow: auto;
}
.more {
  margin: 0;
  padding: var(--space-3) var(--space-4);
}
.list {
  margin: 0;
  padding: var(--space-1) 0;
  list-style: none;
}
li {
  border-bottom: 1px solid var(--border);
}
li.hot {
  background: var(--accent-soft);
}
li.sel {
  background: var(--accent-soft);
  box-shadow: inset 3px 0 0 var(--accent);
}
.row {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  width: 100%;
  padding: var(--space-2) var(--space-4);
  border: 0;
  border-radius: 0;
  background: none;
  color: inherit;
  font-size: 13px;
  text-align: left;
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
.what em {
  color: var(--warn-text);
  font-size: 12px;
  font-style: normal;
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
