<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import type { Api, ModelComponent, Models } from '@/api';
import { t } from '@/lib/i18n';
import { modelAbout, modelLabel } from '@/lib/labels';

// The optional models (ADR-0022, ADR-0023): each downloaded once from its
// publisher and checked against a checksum built into veilq.
const props = defineProps<{ api: Api; compact?: boolean }>();
const emit = defineEmits<{ changed: [] }>();

const models = ref<Models | null>(null);
const error = ref<string | null>(null);
let poll: ReturnType<typeof setInterval> | undefined;

const running = computed(() => models.value?.components.some((c) => c.job?.state === 'running') ?? false);

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

async function load() {
  const before = running.value;
  models.value = await props.api.models();
  if (before && !running.value) emit('changed');
}

async function fetchModel(c: ModelComponent) {
  error.value = null;
  try {
    await props.api.fetchModel(c.name);
    await load();
  } catch (e) {
    error.value = message(e);
  }
}

function megabytes(bytes: number): string {
  return `${(bytes / 2 ** 20).toFixed(1)} MB`;
}

function state(c: ModelComponent): string {
  if (!c.available) return t('models.unavailable');
  if (c.active) return t('models.active');
  if (c.installed) return t('models.restart');
  return t('models.notInstalled');
}

onMounted(async () => {
  try {
    await load();
  } catch (e) {
    error.value = message(e);
  }
  poll = setInterval(() => {
    if (running.value) void load().catch(() => {});
  }, 1000);
});
onBeforeUnmount(() => clearInterval(poll));
</script>

<template>
  <section class="card">
    <h3>{{ t('models.title') }}</h3>
    <p class="hint">{{ t('models.about') }}</p>
    <p v-if="error" class="failure">{{ error }}</p>
    <div v-for="c in models?.components ?? []" :key="c.name" class="model">
      <div class="model-head">
        <strong>{{ modelLabel(c.name) }}</strong>
        <span class="badge" :class="{ on: c.active }">{{ state(c) }}</span>
        <span class="spacer" />
        <button
          v-if="c.available && !c.installed && c.job?.state !== 'running'"
          class="small"
          @click="fetchModel(c)"
        >
          {{ t('models.download') }}
        </button>
      </div>
      <p class="about">{{ modelAbout(c.name, c.about) }}</p>
      <p v-if="c.job?.state === 'running'" class="progress">
        {{ t('models.downloading', { index: c.job.index, of: c.job.of, size: megabytes(c.job.bytes) }) }}
      </p>
      <p v-else-if="c.job?.state === 'failed'" class="failure">{{ c.job.error }}</p>
    </div>
    <p v-if="models && !compact" class="hint mono">{{ models.dir }}</p>
  </section>
</template>

<style scoped>
.model {
  padding: var(--space-2) 0;
  border-top: 1px solid var(--border);
}
.model-head {
  display: flex;
  gap: var(--space-2);
  align-items: center;
}
.about {
  color: var(--text-muted);
  font-size: 12.5px;
}
.badge {
  font-size: 11px;
  padding: 1px 7px;
  border-radius: 99px;
  background: var(--surface-3);
  color: var(--text-2);
}
.badge.on {
  background: var(--ok-soft);
  color: var(--ok-text);
}
.progress {
  font-size: 12.5px;
  color: var(--accent-text);
}
</style>
