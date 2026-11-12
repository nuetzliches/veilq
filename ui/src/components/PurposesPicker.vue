<script setup lang="ts">
import type { PurposeInfo } from '@/api';
import { t } from '@/lib/i18n';
import { entityLabel, purposeAbout, purposeLabel } from '@/lib/labels';

// What veilq is used for (ADR-0040): `general` always, others added to it.
// Each shows what it changes against `general`, so a choice is never a
// black box: thresholds it lowers, patterns it adds, what mistakes cost.
const props = defineProps<{ purposes: PurposeInfo[]; modelValue: string[]; disabled?: boolean }>();
const emit = defineEmits<{ 'update:modelValue': [value: string[]] }>();

function chosen(name: string): boolean {
  return name === 'general' || props.modelValue.includes(name);
}

function toggle(name: string, on: boolean) {
  const rest = props.modelValue.filter((n) => n !== name && n !== 'general');
  emit('update:modelValue', ['general', ...(on ? [...rest, name] : rest)]);
}

/** What `p` changes against `general`, one line each. */
function changes(p: PurposeInfo): string[] {
  const out: string[] = [];
  for (const [entity, rule] of Object.entries(p.entities)) {
    if (rule.min_confidence !== undefined) {
      out.push(t('purposes.threshold', { type: entityLabel(entity), value: rule.min_confidence }));
    }
  }
  const types = [...new Set(p.patterns.map(([, entity]) => entityLabel(entity)))];
  if (p.patterns.length) out.push(t('purposes.patterns', { n: p.patterns.length, types: types.join(', ') }));
  if (p.allow) out.push(t('purposes.allow', { n: p.allow }));
  for (const [entity, c] of Object.entries(p.costs)) {
    out.push(t('purposes.cost', { type: entityLabel(entity), miss: c.miss, fp: c.false_positive }));
  }
  return out;
}
</script>

<template>
  <div class="purposes">
    <label v-for="p in purposes" :key="p.name" class="purpose" :class="{ on: chosen(p.name) }">
      <input
        type="checkbox"
        :checked="chosen(p.name)"
        :disabled="disabled || p.name === 'general'"
        @change="toggle(p.name, ($event.target as HTMLInputElement).checked)"
      />
      <span class="body">
        <strong>{{ purposeLabel(p.name) }}</strong>
        <span class="about">{{ purposeAbout(p.name, p.about) }}</span>
        <span v-if="p.name === 'general'" class="change">{{ t('purposes.always') }}</span>
        <span v-for="c in changes(p)" :key="c" class="change">{{ c }}</span>
      </span>
    </label>
  </div>
</template>

<style scoped>
.purposes {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: var(--space-3);
}
.purpose {
  display: flex;
  gap: var(--space-2);
  align-items: flex-start;
  padding: var(--space-3);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  cursor: pointer;
}
.purpose.on {
  border-color: var(--accent);
}
.body {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.about {
  color: var(--text-2);
  font-size: 13px;
}
.change {
  color: var(--text-muted);
  font-size: 12px;
}
</style>
