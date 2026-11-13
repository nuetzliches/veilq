<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import type { Api, Batch, Info } from '@/api';
import FolderPicker from '@/components/FolderPicker.vue';
import { t } from '@/lib/i18n';
import { actionLabel } from '@/lib/labels';

// A folder run (ADR-0028): every file under a folder, redacted into a new
// folder beside it. What veilq cannot read is listed, never copied; the
// target gets veilq-report.json (types and counts, no values). A folder
// outside the one veilq reads becomes that folder (ADR-0033), as choosing it
// in Settings would.
const props = defineProps<{ api: Api; info: Info; action: string }>();
const emit = defineEmits<{ root: [] }>();

const root = ref(props.info.root);
const source = ref('');
const target = ref('');
const unverified = ref(false);
const picking = ref<'source' | 'target' | null>(null);
const batch = ref<Batch | null>(null);
const error = ref<string | null>(null);
let poll: ReturnType<typeof setInterval> | undefined;

const sep = computed(() => (root.value.includes('\\') ? '\\' : '/'));
const errors = computed(() => batch.value?.files.filter((f) => f.error) ?? []);
const findings = computed(() => batch.value?.files.reduce((n, f) => n + (f.findings ?? 0), 0) ?? 0);
const reviews = computed(() => batch.value?.files.filter((f) => f.review).length ?? 0);

/** The source as the server takes it: relative to the root, `/`-separated. */
function relative(abs: string): string | null {
  const base = root.value.replace(/[\\/]+$/, '');
  if (abs.toLowerCase() === base.toLowerCase()) return '';
  if (!abs.toLowerCase().startsWith((base + sep.value).toLowerCase())) return null;
  return abs.slice(base.length + 1).replaceAll('\\', '/');
}

async function pickSource(path: string) {
  picking.value = null;
  error.value = null;
  if (relative(path) === null) {
    try {
      root.value = (await props.api.setRoot(path)).root;
      emit('root');
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
      return;
    }
  }
  source.value = path;
  target.value = `${path.replace(/[\\/]+$/, '')}-${t('folder.suffix')}`;
}

async function refresh() {
  batch.value = await props.api.batch();
}

async function start() {
  const rel = relative(source.value);
  if (rel === null || !target.value) return;
  error.value = null;
  try {
    batch.value = await props.api.startBatch({
      source: rel,
      target: target.value,
      action: props.action || undefined,
      unverified: unverified.value || undefined,
    });
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  }
}

onMounted(async () => {
  await refresh().catch(() => {});
  poll = setInterval(() => {
    if (batch.value?.running) void refresh().catch(() => {});
  }, 700);
});
onBeforeUnmount(() => clearInterval(poll));
</script>

<template>
  <div class="run">
    <section class="card">
      <p class="hint">{{ t('folder.about') }}</p>
      <div class="row">
        <span class="label">{{ t('folder.from') }}</span>
        <span class="mono path">{{ source || t('folder.none') }}</span>
        <button class="small" :class="{ primary: !source }" @click="picking = 'source'">{{ t('common.choose') }}</button>
      </div>
      <div class="row">
        <span class="label">{{ t('folder.to') }}</span>
        <input v-model="target" class="mono path" :placeholder="t('folder.newFolder')" />
        <button class="small" @click="picking = 'target'">{{ t('common.choose') }}</button>
      </div>
      <details class="more">
        <summary>{{ t('folder.more') }}</summary>
        <label class="check"><input v-model="unverified" type="checkbox" /> {{ t('folder.unverified') }}</label>
      </details>
      <div class="row">
        <button class="primary" :disabled="!source || !target || batch?.running" @click="start">
          {{ t('folder.start') }}
        </button>
        <button v-if="batch?.running" @click="api.cancelBatch()">{{ t('folder.stop') }}</button>
        <span v-if="!source" class="hint">{{ t('folder.needSource') }}</span>
        <span v-else-if="!target" class="hint">{{ t('folder.needTarget') }}</span>
        <span v-else class="hint">{{ t('output.replaceWith') }} {{ actionLabel(action || info.default_action) }}</span>
      </div>
      <p v-if="error" class="failure">{{ error }}</p>
    </section>

    <section v-if="batch" class="card">
      <h3>{{ batch.running ? t('folder.running') : batch.cancelled ? t('folder.stopped') : t('folder.done') }}</h3>
      <progress :max="batch.total || 1" :value="batch.done" />
      <p class="hint">
        {{ t('folder.progress', { done: batch.done, total: batch.total, findings }) }} ·
        <span :class="{ bad: errors.length }">{{ t('folder.failed', { n: errors.length }) }}</span>
        <template v-if="reviews"> · {{ t('folder.reviews', { n: reviews }) }}</template>
        <template v-if="batch.current"> · <span class="mono">{{ batch.current }}</span></template>
      </p>
      <p v-if="!batch.running" class="hint">
        {{ t('folder.written') }} <span class="mono">{{ batch.target }}</span>
      </p>
      <table v-if="batch.files.length" class="files">
        <tbody>
          <tr v-for="f in [...batch.files].reverse()" :key="f.path" :class="{ bad: f.error, review: f.review }">
            <td class="mono">{{ f.path }}</td>
            <td v-if="f.error" class="err">{{ f.error }}</td>
            <td v-else>
              {{ f.findings }} · {{ (f.types ?? []).join(', ') }}<template v-if="f.review"> · {{ t('folder.review') }}</template>
            </td>
          </tr>
        </tbody>
      </table>
    </section>

    <FolderPicker
      v-if="picking"
      :api="api"
      :title="picking === 'source' ? t('folder.pickSource') : t('folder.pickTarget')"
      :start="picking === 'source' ? root : ''"
      :action="picking === 'source' ? t('folder.pickSourceAction') : t('folder.pickTargetAction')"
      @pick="picking === 'source' ? pickSource($event) : ((target = `${$event}`), (picking = null))"
      @close="picking = null"
    />
  </div>
</template>

<style scoped>
.run {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  padding: var(--space-4);
}
.card {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-4);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--surface);
  font-size: 13px;
}
.card h3 {
  font-size: 14px;
}
.row {
  display: flex;
  gap: var(--space-2);
  align-items: center;
}
.label {
  width: 3em;
  color: var(--text-muted);
}
.path {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
input.path {
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: var(--surface-2);
  padding: 3px 6px;
}
.more summary {
  cursor: pointer;
  color: var(--text-2);
}
.check {
  margin-top: var(--space-2);
  display: flex;
  gap: var(--space-2);
  align-items: center;
  color: var(--text-2);
}
.hint {
  color: var(--text-muted);
  font-size: 12.5px;
}
progress {
  width: 100%;
}
.files {
  border-collapse: collapse;
  font-size: 12.5px;
}
.files td {
  padding: 2px var(--space-2) 2px 0;
  border-bottom: 1px solid var(--border);
}
.bad,
.err {
  color: var(--danger-text);
}
.review td:last-child {
  color: var(--warn-text);
}
.failure {
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-sm);
  background: var(--danger-soft);
  color: var(--danger-text);
}
</style>
