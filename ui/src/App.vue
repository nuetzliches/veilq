<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { createApi, type Info, type Redaction, type Source } from '@/api';
import { inputSpans } from '@/lib/segments';
import ComparePane from '@/components/ComparePane.vue';
import FileBrowser from '@/components/FileBrowser.vue';
import FindingsTable from '@/components/FindingsTable.vue';

type Kind = Source['kind'];

const api = createApi();
const info = ref<Info | null>(null);
const infoError = ref<string | null>(null);

const kind = ref<Kind>('text');
const text = ref('');
const editing = ref(true);
const selectedFile = ref<string | null>(null);
const dragging = ref(false);

const busy = ref(false);
const result = ref<Redaction | null>(null);
const error = ref<string | null>(null);
/** The source of the last run, shown above the input while it runs or failed. */
const sourceName = ref<string | null>(null);
const hovered = ref<number | null>(null);
const copied = ref(false);

const findings = computed(() => result.value?.report.findings ?? []);
const spansIn = computed(() => inputSpans(findings.value));

const tabs: { kind: Kind; label: string }[] = [
  { kind: 'text', label: 'Text' },
  { kind: 'upload', label: 'Upload' },
  { kind: 'file', label: 'File' },
];

async function run(source: Source, name: string | null) {
  busy.value = true;
  error.value = null;
  result.value = null;
  hovered.value = null;
  sourceName.value = name;
  try {
    result.value = await api.redact(source);
  } catch (e) {
    // Fail closed: no output at all when veilq could not redact.
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    busy.value = false;
  }
}

function redactText() {
  if (!text.value) return;
  editing.value = false;
  void run({ kind: 'text', text: text.value }, null);
}

function editText() {
  editing.value = true;
  result.value = null;
  error.value = null;
}

function selectTab(k: Kind) {
  kind.value = k;
  result.value = null;
  error.value = null;
  sourceName.value = null;
  if (k === 'text') editing.value = true;
}

function selectFile(path: string) {
  selectedFile.value = path;
  void run({ kind: 'file', path }, path);
}

async function upload(file: File | undefined) {
  if (!file) return;
  if (info.value && file.size > info.value.max_input) {
    result.value = null;
    sourceName.value = file.name;
    error.value = `${file.name} is larger than ${info.value.max_input >> 20} MiB; use \`veilq redact\``;
    return;
  }
  const data = await toBase64(file);
  void run({ kind: 'upload', name: file.name, data }, file.name);
}

function toBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).replace(/^data:[^,]*,/, ''));
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

function onDrop(event: DragEvent) {
  dragging.value = false;
  void upload(event.dataTransfer?.files[0]);
}

function onPick(event: Event) {
  const input = event.target as HTMLInputElement;
  void upload(input.files?.[0]);
  input.value = '';
}

async function copyOutput() {
  if (!result.value) return;
  await navigator.clipboard.writeText(result.value.output);
  copied.value = true;
  setTimeout(() => (copied.value = false), 1200);
}

function downloadOutput() {
  if (!result.value) return;
  const base = (result.value.name ?? 'text.txt').split('/').pop() ?? 'text.txt';
  const dot = base.lastIndexOf('.');
  const name = dot > 0 ? `${base.slice(0, dot)}.redacted${base.slice(dot)}` : `${base}.redacted`;
  const url = URL.createObjectURL(new Blob([result.value.output], { type: 'text/plain;charset=utf-8' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  a.click();
  URL.revokeObjectURL(url);
}

onMounted(async () => {
  try {
    info.value = await api.info();
  } catch (e) {
    infoError.value = e instanceof Error ? e.message : String(e);
  }
});
</script>

<template>
  <div class="app">
    <header class="topbar">
      <div class="brand">
        <img src="/favicon.svg" alt="" width="22" height="22" />
        <span>veilq</span>
        <span v-if="api.mock" class="badge warn" title="An in-browser fake with synthetic data, not veilq">mock</span>
      </div>
      <dl v-if="info" class="meta">
        <div><dt>root</dt><dd class="mono">{{ info.root }}</dd></div>
        <div><dt>policy</dt><dd class="mono">{{ info.policy }}</dd></div>
        <div><dt>action</dt><dd class="mono">{{ info.default_action }}</dd></div>
        <div><dt>version</dt><dd class="mono">{{ info.version }}</dd></div>
      </dl>
      <p v-else-if="infoError" class="meta error">{{ infoError }}</p>
    </header>

    <aside class="sidebar">
      <div class="tabs" role="tablist">
        <button
          v-for="t in tabs"
          :key="t.kind"
          role="tab"
          :aria-selected="kind === t.kind"
          :class="{ active: kind === t.kind }"
          @click="selectTab(t.kind)"
        >
          {{ t.label }}
        </button>
      </div>

      <section v-if="kind === 'text'" class="source">
        <p class="hint">Paste or type into the input pane, then redact.</p>
        <button class="primary" :disabled="!text || busy" @click="redactText">Redact <kbd>Ctrl+Enter</kbd></button>
        <button v-if="!editing" @click="editText">Edit the text</button>
      </section>

      <section v-else-if="kind === 'upload'" class="source">
        <label
          class="drop"
          :class="{ over: dragging }"
          @dragover.prevent="dragging = true"
          @dragleave="dragging = false"
          @drop.prevent="onDrop"
        >
          <input type="file" @change="onPick" />
          <strong>Drop a file</strong>
          <span>or click to choose one. It is sent to veilq on this machine only.</span>
        </label>
        <p class="hint">Text files until M3; documents and images are refused, not passed through.</p>
      </section>

      <section v-else class="source browser">
        <FileBrowser :api="api" :selected="selectedFile" @select="selectFile" />
      </section>
    </aside>

    <main class="compare">
      <section class="pane">
        <header class="pane-head">
          <h2>Input</h2>
          <span v-if="result?.name ?? sourceName" class="mono name">{{ result?.name ?? sourceName }}</span>
        </header>
        <div class="pane-body">
          <textarea
            v-if="kind === 'text' && editing"
            v-model="text"
            class="editor"
            spellcheck="false"
            placeholder="Text with personal data …"
            @keydown.ctrl.enter="redactText"
            @keydown.meta.enter="redactText"
          />
          <ComparePane
            v-else-if="result"
            side="input"
            :text="result.input"
            :spans="spansIn"
            :findings="findings"
            :hovered="hovered"
            @hover="hovered = $event"
          />
          <ComparePane
            v-else-if="kind === 'text'"
            side="input"
            :text="text"
            :spans="[]"
            :findings="[]"
            :hovered="null"
          />
          <p v-else class="placeholder">{{ busy ? 'Reading …' : 'Choose a source on the left.' }}</p>
        </div>
      </section>

      <section class="pane">
        <header class="pane-head">
          <h2>Output</h2>
          <span v-if="result" class="count">{{ findings.length }} finding{{ findings.length === 1 ? '' : 's' }}</span>
          <span class="spacer" />
          <button v-if="result" class="small" @click="copyOutput">{{ copied ? 'Copied' : 'Copy' }}</button>
          <button v-if="result" class="small" @click="downloadOutput">Download</button>
        </header>
        <div class="pane-body">
          <div v-if="error" class="failure">
            <strong>No output.</strong>
            <p>{{ error }}</p>
          </div>
          <ComparePane
            v-else-if="result"
            side="output"
            :text="result.output"
            :spans="result.output_spans"
            :findings="findings"
            :hovered="hovered"
            @hover="hovered = $event"
          />
          <p v-else class="placeholder">{{ busy ? 'Redacting …' : 'The redacted output appears here.' }}</p>
        </div>
      </section>
    </main>

    <section class="report">
      <div v-if="result?.report.review_required" class="review">
        <strong>Review required.</strong>
        <span v-for="(r, i) in result.report.review_reasons ?? []" :key="i">{{ r }}</span>
      </div>
      <FindingsTable v-if="findings.length" :findings="findings" :hovered="hovered" @hover="hovered = $event" />
      <p v-else-if="result" class="placeholder">No findings.</p>
    </section>
  </div>
</template>

<style scoped>
.app {
  display: grid;
  grid-template-columns: 280px 1fr;
  grid-template-rows: var(--topbar-h) minmax(0, 1fr) minmax(0, 32vh);
  grid-template-areas:
    'top top'
    'side main'
    'side report';
  height: 100vh;
}
.topbar {
  grid-area: top;
  display: flex;
  align-items: center;
  gap: var(--space-5);
  padding: 0 var(--space-4);
  background: var(--bg-elev);
  border-bottom: 1px solid var(--border);
}
.brand {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-weight: 600;
  font-size: 16px;
}
.meta {
  display: flex;
  gap: var(--space-4);
  margin: 0;
  min-width: 0;
  overflow: hidden;
  font-size: 12.5px;
}
.meta div {
  display: flex;
  gap: 6px;
  min-width: 0;
}
.meta dt {
  color: var(--text-muted);
}
.meta dd {
  margin: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sidebar {
  grid-area: side;
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--border);
  background: var(--surface);
  min-height: 0;
}
.tabs {
  display: flex;
  gap: 2px;
  padding: var(--space-2);
  border-bottom: 1px solid var(--border);
}
.tabs button {
  flex: 1;
  border: 0;
  background: none;
  padding: 6px 4px;
  border-radius: var(--radius-sm);
  font-size: 12.5px;
  cursor: pointer;
  color: var(--text-2);
}
.tabs button.active {
  background: var(--accent-soft);
  color: var(--accent-text);
  font-weight: 500;
}
.source {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-3);
  min-height: 0;
}
.source.browser {
  padding: 0;
  flex: 1;
}
.hint {
  color: var(--text-muted);
  font-size: 12.5px;
}
.drop {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  padding: var(--space-5) var(--space-3);
  border: 1.5px dashed var(--border-strong);
  border-radius: var(--radius);
  text-align: center;
  cursor: pointer;
  font-size: 13px;
  color: var(--text-2);
}
.drop.over {
  border-color: var(--accent);
  background: var(--accent-soft);
}
.drop input {
  display: none;
}
.compare {
  grid-area: main;
  display: grid;
  grid-template-columns: 1fr 1fr;
  min-height: 0;
}
.pane {
  display: flex;
  flex-direction: column;
  min-height: 0;
  min-width: 0;
}
.pane + .pane {
  border-left: 1px solid var(--border);
}
.pane-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  height: 40px;
  padding: 0 var(--space-4);
  border-bottom: 1px solid var(--border);
  background: var(--surface-2);
}
.pane-head h2 {
  font-size: 13px;
}
.name,
.count {
  color: var(--text-muted);
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.spacer {
  flex: 1;
}
.pane-body {
  flex: 1;
  overflow: auto;
  background: var(--surface);
}
.editor {
  width: 100%;
  height: 100%;
  border: 0;
  resize: none;
  padding: var(--space-4);
  background: transparent;
  font-family: var(--font-mono);
  font-size: 12.5px;
  line-height: 1.6;
}
.editor:focus {
  outline: none;
}
.placeholder {
  color: var(--text-faint);
  padding: var(--space-4);
  font-size: 13px;
}
.failure {
  margin: var(--space-4);
  padding: var(--space-3) var(--space-4);
  border-radius: var(--radius);
  background: var(--danger-soft);
  color: var(--danger-text);
  font-size: 13px;
}
.report {
  grid-area: report;
  overflow: auto;
  border-top: 1px solid var(--border);
  background: var(--surface);
}
.review {
  display: flex;
  flex-wrap: wrap;
  gap: 0 var(--space-2);
  padding: var(--space-2) var(--space-4);
  background: var(--warn-soft);
  color: var(--warn-text);
  font-size: 12.5px;
}
.error {
  color: var(--danger-text);
}
.badge {
  font-size: 11px;
  font-weight: 500;
  padding: 1px 7px;
  border-radius: 99px;
}
.badge.warn {
  background: var(--warn-soft);
  color: var(--warn-text);
}
kbd {
  font-size: 11px;
  opacity: 0.75;
  margin-left: 4px;
}
@media (max-width: 900px) {
  .app {
    grid-template-columns: 1fr;
    grid-template-rows: var(--topbar-h) auto minmax(50vh, 1fr) auto;
    grid-template-areas: 'top' 'side' 'main' 'report';
    height: auto;
  }
}
</style>
