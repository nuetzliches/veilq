<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import type { Api, ModelComponent } from '@/api';
import { t } from '@/lib/i18n';
import { modelLabel } from '@/lib/labels';

// A model a message asks for, offered where the message is: one click
// downloads it (checked against the checksum built into veilq, ADR-0022),
// and the source runs again once the engine uses it. Until then nothing is
// passed through: the run stays refused, or flagged for review.
const props = defineProps<{ api: Api; names: string[] }>();
const emit = defineEmits<{ ready: [] }>();

const components = ref<ModelComponent[]>([]);
const error = ref<string | null>(null);
let poll: ReturnType<typeof setInterval> | undefined;

const shown = computed(() => components.value.filter((c) => props.names.includes(c.name) && c.available && !c.active));
const running = computed(() => shown.value.some((c) => c.job?.state === 'running'));

async function load() {
  const wasRunning = running.value;
  components.value = (await props.api.models()).components;
  const missing = components.value.filter((c) => props.names.includes(c.name) && c.available && !c.active);
  if (wasRunning && !running.value && !missing.length) emit('ready');
}

async function fetch(c: ModelComponent) {
  error.value = null;
  try {
    await props.api.fetchModel(c.name);
    await load();
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  }
}

function megabytes(bytes: number): string {
  return `${(bytes / 2 ** 20).toFixed(0)} MB`;
}

onMounted(async () => {
  await load().catch((e) => (error.value = e instanceof Error ? e.message : String(e)));
  poll = setInterval(() => {
    if (running.value) void load().catch(() => {});
  }, 1000);
});
onBeforeUnmount(() => clearInterval(poll));
</script>

<template>
  <div v-if="shown.length || error" class="missing">
    <p>{{ t('models.missing') }}</p>
    <div v-for="c in shown" :key="c.name" class="row">
      <strong>{{ modelLabel(c.name) }}</strong>
      <span class="spacer" />
      <span v-if="c.job?.state === 'running'" class="progress">
        {{ t('models.downloading', { index: c.job.index, of: c.job.of, size: megabytes(c.job.bytes) }) }}
      </span>
      <span v-else-if="c.installed" class="hint">{{ t('models.restart') }}</span>
      <button v-else class="small primary" @click="fetch(c)">{{ t('models.download') }}</button>
    </div>
    <p v-for="c in shown.filter((c) => c.job?.state === 'failed')" :key="`e-${c.name}`" class="error">
      {{ c.job?.state === 'failed' ? c.job.error : '' }}
    </p>
    <p v-if="error" class="error">{{ error }}</p>
    <p class="hint">{{ t('models.privacy') }}</p>
  </div>
</template>

<style scoped>
.missing {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  margin: var(--space-4);
  padding: var(--space-3) var(--space-4);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--surface-2);
  font-size: 13px;
}
.row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.spacer {
  flex: 1;
}
.progress {
  color: var(--accent-text);
  font-size: 12.5px;
}
.hint {
  color: var(--text-muted);
  font-size: 12px;
}
.error {
  color: var(--danger-text);
}
</style>
