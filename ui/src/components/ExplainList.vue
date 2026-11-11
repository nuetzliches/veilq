<script setup lang="ts">
import type { Explained, Reading, Verdict } from '@/api';
import { t } from '@/lib/i18n';
import { entityLabel } from '@/lib/labels';

// What veilq read and why it decided (ADR-0034): each text it read (a line,
// a cell, a picture's OCR line) with every candidate and the verdict on it.
// The values are in clear, so the list says so before anything else.
defineProps<{ readings: Reading[] }>();

/** How far below its threshold a candidate is a near miss. */
const NEAR = 0.2;

function near(c: Explained): boolean {
  return c.verdict === 'below_threshold' && c.confidence >= c.threshold - NEAR;
}

function verdictLabel(v: Verdict): string {
  return t(`verdict.${v}` as const);
}

function percent(x: number): number {
  return Math.round(x * 100);
}
</script>

<template>
  <section class="explain">
    <p class="warning">{{ t('explain.warning') }}</p>
    <article v-for="(r, i) in readings" :key="i" class="reading">
      <pre class="text">{{ r.text }}</pre>
      <p v-if="r.context" class="context">{{ t('explain.context', { context: r.context.replace(/\n/g, ' | ') }) }}</p>
      <ul v-if="r.candidates.length" class="candidates">
        <li v-for="(c, j) in r.candidates" :key="j" :class="[c.verdict, { near: near(c) }]">
          <span class="verdict">{{ verdictLabel(c.verdict) }}</span>
          <div class="what">
            <strong>{{ entityLabel(c.entity) }}</strong>
            <code>{{ c.value }}</code>
            <span class="score">
              {{ t('explain.score', { confidence: percent(c.confidence), threshold: percent(c.threshold), detector: c.detector }) }}
            </span>
            <span v-if="c.raised" class="note">{{ t('explain.raised') }}</span>
            <span v-if="near(c)" class="note">{{ t('explain.near') }}</span>
          </div>
        </li>
      </ul>
      <p v-else class="none">{{ t('explain.nothing') }}</p>
    </article>
  </section>
</template>

<style scoped>
.explain {
  border-top: 1px solid var(--border);
}
.warning {
  margin: 0;
  padding: var(--space-2) var(--space-4);
  background: var(--warn-soft);
  color: var(--warn-text);
  font-size: 12px;
}
.reading {
  padding: var(--space-2) var(--space-4);
  border-bottom: 1px solid var(--border);
}
.text {
  margin: 0;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-2);
}
.context,
.none {
  margin: var(--space-1) 0 0;
  color: var(--text-muted);
  font-size: 11.5px;
}
.candidates {
  margin: var(--space-1) 0 0;
  padding: 0;
  list-style: none;
}
.candidates li {
  display: flex;
  gap: var(--space-2);
  padding: 2px 0;
  font-size: 12px;
}
.verdict {
  flex: none;
  min-width: 9em;
  color: var(--text-muted);
}
li.kept .verdict {
  color: var(--accent-text);
  font-weight: 600;
}
li.near .verdict {
  color: var(--danger-text);
}
.what {
  display: flex;
  flex-wrap: wrap;
  gap: 0 var(--space-2);
  min-width: 0;
}
.what code {
  word-break: break-all;
}
.score,
.note {
  color: var(--text-muted);
  font-size: 11.5px;
}
</style>
