<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import type { Api, Batch, Info } from '@/api';
import FolderPicker from '@/components/FolderPicker.vue';

// A folder run (ADR-0028): every file under a folder of the root, redacted
// into a new folder beside it. What veilq cannot read is listed, never
// copied; the target gets veilq-report.json (types and counts, no values).
const props = defineProps<{ api: Api; info: Info; action: string }>();

const source = ref('');
const target = ref('');
const unverified = ref(false);
const picking = ref<'source' | 'target' | null>(null);
const batch = ref<Batch | null>(null);
const error = ref<string | null>(null);
let poll: ReturnType<typeof setInterval> | undefined;

const sep = computed(() => (props.info.root.includes('\\') ? '\\' : '/'));
const errors = computed(() => batch.value?.files.filter((f) => f.error) ?? []);
const findings = computed(() => batch.value?.files.reduce((n, f) => n + (f.findings ?? 0), 0) ?? 0);
const reviews = computed(() => batch.value?.files.filter((f) => f.review).length ?? 0);

/** The source as the server takes it: relative to the root, `/`-separated. */
function relative(abs: string): string | null {
  const root = props.info.root.replace(/[\\/]+$/, '');
  if (abs.toLowerCase() === root.toLowerCase()) return '';
  if (!abs.toLowerCase().startsWith((root + sep.value).toLowerCase())) return null;
  return abs.slice(root.length + 1).replaceAll('\\', '/');
}

function pickSource(path: string) {
  picking.value = null;
  if (relative(path) === null) {
    error.value = `${path} is not under the folder veilq reads (${props.info.root}); choose that in Settings.`;
    return;
  }
  error.value = null;
  source.value = path;
  target.value = `${path.replace(/[\\/]+$/, '')}-redacted`;
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
      <h3>Redact a whole folder</h3>
      <p class="hint">
        Every file under the folder is redacted into a new folder; the originals stay as they are. A file veilq cannot
        read is listed, not copied.
      </p>
      <div class="row">
        <span class="label">From</span>
        <span class="mono path">{{ source || 'no folder chosen' }}</span>
        <button class="small" @click="picking = 'source'">Choose …</button>
      </div>
      <div class="row">
        <span class="label">To</span>
        <input v-model="target" class="mono path" placeholder="a new folder" />
        <button class="small" @click="picking = 'target'">Choose …</button>
      </div>
      <label class="check">
        <input v-model="unverified" type="checkbox" /> Write pictures that do not settle anyway, for a review (ADR-0026)
      </label>
      <div class="row">
        <button class="primary" :disabled="!source || !target || batch?.running" @click="start">Redact folder</button>
        <button v-if="batch?.running" @click="api.cancelBatch()">Stop</button>
        <span class="hint">Action: {{ action || `policy (${info.default_action})` }}</span>
      </div>
      <p v-if="error" class="failure">{{ error }}</p>
    </section>

    <section v-if="batch" class="card">
      <h3>{{ batch.running ? 'Redacting …' : batch.cancelled ? 'Stopped' : 'Done' }}</h3>
      <progress :max="batch.total || 1" :value="batch.done" />
      <p class="hint">
        {{ batch.done }} of {{ batch.total }} files · {{ findings }} findings ·
        <span :class="{ bad: errors.length }">{{ errors.length }} not redacted</span>
        <template v-if="reviews"> · {{ reviews }} to review</template>
        <template v-if="batch.current"> · <span class="mono">{{ batch.current }}</span></template>
      </p>
      <p v-if="!batch.running" class="hint">
        Written to <span class="mono">{{ batch.target }}</span>, with veilq-report.json.
      </p>
      <table v-if="batch.files.length" class="files">
        <tbody>
          <tr v-for="f in [...batch.files].reverse()" :key="f.path" :class="{ bad: f.error, review: f.review }">
            <td class="mono">{{ f.path }}</td>
            <td v-if="f.error" class="err">{{ f.error }}</td>
            <td v-else>{{ f.findings }} · {{ (f.types ?? []).join(', ') }}<template v-if="f.review"> · review</template></td>
          </tr>
        </tbody>
      </table>
    </section>

    <FolderPicker
      v-if="picking"
      :api="api"
      :title="picking === 'source' ? 'The folder to redact' : 'Where the redacted files go'"
      :start="picking === 'source' ? info.root : ''"
      :action="picking === 'source' ? 'Redact this folder' : 'Write into this folder'"
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
  overflow: auto;
  height: 100%;
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
.check {
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
