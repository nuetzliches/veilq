<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { ApiError, createApi, type Info, type Redaction, type Source } from '@/api';
import BrandMark from '@/components/BrandMark.vue';
import ComparePane from '@/components/ComparePane.vue';
import FileBrowser from '@/components/FileBrowser.vue';
import FindingsTable from '@/components/FindingsTable.vue';
import ImagePane from '@/components/ImagePane.vue';
import ImageViewer, { type Pictures } from '@/components/ImageViewer.vue';
import { pastedName } from '@/lib/paste';

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
/** The last run failed because a picture's verify rounds did not settle: it can be written anyway. */
const unsettled = ref(false);
/** The source of the last run, shown above the input while it runs or failed. */
const sourceName = ref<string | null>(null);
const hovered = ref<number | null>(null);
const copied = ref(false);
/** The action of a run: `''` for the policy's, else one of `info.actions`. */
const action = ref('');
/** An image or a PDF's pages, shown as soon as chosen, before the redaction is back. */
const preview = ref<{ pages: string[]; mediaType: string; pageCount?: number } | null>(null);
/** The last run's source, run again when the action changes. */
const lastRun = ref<{ source: Source; name: string | null } | null>(null);

const findings = computed(() => result.value?.report.findings ?? []);
/** An image the browser shows: set side by side. */
const image = computed(() => {
  const r = result.value;
  return r?.media_type?.startsWith('image/') && r.input_base64 && r.output_base64 ? r : null;
});
/** A PDF: its pages side by side, as the server rendered them. */
const pdf = computed(() => {
  const r = result.value;
  return r?.input_pages && r.output_pages ? r : null;
});

/** The side the viewer shows, or none. */
const viewer = ref<'input' | 'output' | null>(null);
/** What each side shows as pictures, for the viewer. */
const inputPictures = computed<Pictures | null>(() => {
  if (image.value) return { pages: [image.value.input_base64!], mediaType: image.value.media_type!, paged: false };
  if (pdf.value) return { pages: pdf.value.input_pages!, mediaType: 'image/png', paged: true };
  if (!result.value && preview.value)
    return { pages: preview.value.pages, mediaType: preview.value.mediaType, paged: !!preview.value.pageCount };
  return null;
});
const outputPictures = computed<Pictures | null>(() => {
  if (error.value) return null;
  if (image.value) return { pages: [image.value.output_base64!], mediaType: image.value.media_type!, paged: false };
  if (pdf.value) return { pages: pdf.value.output_pages!, mediaType: 'image/png', paged: true };
  return null;
});

const tabs: { kind: Kind; label: string }[] = [
  { kind: 'text', label: 'Text' },
  { kind: 'upload', label: 'Upload' },
  { kind: 'file', label: 'File' },
];

async function run(source: Source, name: string | null, unverified = false) {
  busy.value = true;
  viewer.value = null;
  error.value = null;
  unsettled.value = false;
  result.value = null;
  hovered.value = null;
  sourceName.value = name;
  lastRun.value = { source, name };
  try {
    result.value = await api.redact(source, action.value || undefined, unverified);
  } catch (e) {
    // Fail closed: no output at all when veilq could not redact.
    error.value = e instanceof Error ? e.message : String(e);
    unsettled.value = e instanceof ApiError && e.status === 409;
  } finally {
    busy.value = false;
  }
}

/** Runs the last source again, writing a picture whose verify did not settle. */
function runUnverified() {
  if (lastRun.value) void run(lastRun.value.source, lastRun.value.name, true);
}

function changeAction() {
  if (lastRun.value && !editing.value) void run(lastRun.value.source, lastRun.value.name);
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
  preview.value = null;
  result.value = null;
  error.value = null;
  sourceName.value = null;
  if (k === 'text') editing.value = true;
}

const PICTURE = /\.(png|jpe?g|webp|gif)$/i;
const PDF = /\.pdf$/i;

function selectFile(path: string) {
  selectedFile.value = path;
  preview.value = null;
  if (PICTURE.test(path) || PDF.test(path)) {
    api
      .picture(path)
      .then((p) => {
        if (selectedFile.value === path && !result.value) {
          preview.value = { pages: p.pages, mediaType: p.media_type, pageCount: p.page_count };
        }
      })
      .catch(() => {});
  }
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
  preview.value =
    PICTURE.test(file.name) && file.type.startsWith('image/') ? { pages: [data], mediaType: file.type } : null;
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

/**
 * Ctrl+V with a file or an image in the clipboard (a screenshot) uploads it,
 * as a drop would. Text pasted into an editable field stays plain pasting.
 */
function onPaste(event: ClipboardEvent) {
  const data = event.clipboardData;
  const target = event.target as HTMLElement | null;
  const editable = target?.closest('textarea, input, [contenteditable="true"]');
  if (!data || (editable && data.types.includes('text/plain'))) return;
  const file = Array.from(data.items)
    .find((i) => i.kind === 'file')
    ?.getAsFile();
  if (!file) return;
  event.preventDefault();
  if (busy.value) return;
  selectTab('upload');
  void upload(new File([file], pastedName(file.name, file.type), { type: file.type }));
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

function redactedName(): string {
  const base = (result.value?.name ?? 'text.txt').split('/').pop() ?? 'text.txt';
  const dot = base.lastIndexOf('.');
  return dot > 0 ? `${base.slice(0, dot)}.redacted${base.slice(dot)}` : `${base}.redacted`;
}

function downloadOutput() {
  if (!result.value) return;
  const name = redactedName();
  const data = result.value.output_base64;
  const blob = data
    ? new Blob([Uint8Array.from(atob(data), (c) => c.charCodeAt(0))], { type: 'application/octet-stream' })
    : new Blob([result.value.output], { type: 'text/plain;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  a.click();
  URL.revokeObjectURL(url);
}

onMounted(async () => {
  window.addEventListener('paste', onPaste);
  try {
    info.value = await api.info();
  } catch (e) {
    infoError.value = e instanceof Error ? e.message : String(e);
  }
});

onBeforeUnmount(() => window.removeEventListener('paste', onPaste));
</script>

<template>
  <div class="app">
    <header class="topbar">
      <div class="brand">
        <BrandMark />
        <span>veilq</span>
        <span v-if="api.mock" class="badge warn" title="An in-browser fake with synthetic data, not veilq">mock</span>
      </div>
      <dl v-if="info" class="meta">
        <div><dt>root</dt><dd class="mono">{{ info.root }}</dd></div>
        <div><dt>policy</dt><dd class="mono">{{ info.policy }}</dd></div>
        <div>
          <dt><label for="action">action</label></dt>
          <dd>
            <select id="action" v-model="action" class="mono" :disabled="busy" @change="changeAction">
              <option value="">policy ({{ info.default_action }})</option>
              <option v-for="a in info.actions" :key="a" :value="a">{{ a }}</option>
            </select>
          </dd>
        </div>
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
          <span>or click to choose one, or paste an image with <kbd>Ctrl+V</kbd>. It is sent to veilq on this machine only.</span>
        </label>
        <p class="hint">Text, logs, CSV, JSON, Word, Excel, PowerPoint, mails, images (PNG, JPEG, WebP, GIF) and PDF. Images and PDF need OCR (`veilq models fetch ocr`, for PDF also `pdf`); without it they are refused, not passed through.</p>
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
          <span class="spacer" />
          <button v-if="inputPictures" class="small" title="Zoom and fullscreen (or double-click the image)" @click="viewer = 'input'">Zoom</button>
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
          <ImagePane
            v-else-if="image"
            side="input"
            :data="image.input_base64!"
            :media-type="image.media_type!"
            :findings="findings"
            :hovered="hovered"
            @hover="hovered = $event"
            @open="viewer = 'input'"
          />
          <div v-else-if="pdf" class="pages">
            <ImagePane
              v-for="(p, i) in pdf.input_pages"
              :key="i"
              side="input"
              :data="p"
              media-type="image/png"
              :page="i + 1"
              :findings="findings"
              :hovered="hovered"
              @hover="hovered = $event"
              @open="viewer = 'input'"
            />
            <p v-if="(pdf.page_count ?? 0) > pdf.input_pages!.length" class="hint">
              Pages {{ pdf.input_pages!.length + 1 }} to {{ pdf.page_count }} are in the download.
            </p>
          </div>
          <div v-else-if="result?.output_base64" class="binary">
            <strong>{{ result.name }}</strong>
            <p>This file has no text to set side by side here: its findings are listed below, by part and paragraph, sheet and cell, or page and region.</p>
          </div>
          <ComparePane
            v-else-if="result"
            side="input"
            :text="result.input"
            :spans="result.input_spans"
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
          <div v-else-if="preview" class="pages">
            <ImagePane
              v-for="(p, i) in preview.pages"
              :key="i"
              side="input"
              :data="p"
              :media-type="preview.mediaType"
              :page="preview.pageCount ? i + 1 : undefined"
              :findings="[]"
              :hovered="null"
              @open="viewer = 'input'"
            />
          </div>
          <p v-else class="placeholder">{{ busy ? 'Reading …' : 'Choose a source on the left.' }}</p>
        </div>
      </section>

      <section class="pane">
        <header class="pane-head">
          <h2>Output</h2>
          <span v-if="result" class="count">{{ findings.length }} finding{{ findings.length === 1 ? '' : 's' }}</span>
          <span class="spacer" />
          <button v-if="outputPictures" class="small" title="Zoom and fullscreen (or double-click the image)" @click="viewer = 'output'">Zoom</button>
          <button v-if="result && !result.output_base64" class="small" @click="copyOutput">{{ copied ? 'Copied' : 'Copy' }}</button>
          <button v-if="result" class="small" @click="downloadOutput">Download</button>
        </header>
        <div class="pane-body">
          <div v-if="error" class="failure">
            <strong>No output.</strong>
            <p>{{ error }}</p>
            <template v-if="unsettled">
              <p class="unsettled">
                Each round of reading the blacked-out picture again found something new. Generating anyway fills
                that too, but the picture is not read again: look at it before you pass it on.
              </p>
              <button :disabled="busy" @click="runUnverified">Generate anyway</button>
            </template>
          </div>
          <ImagePane
            v-else-if="image"
            side="output"
            :data="image.output_base64!"
            :media-type="image.media_type!"
            :findings="findings"
            :hovered="hovered"
            @hover="hovered = $event"
            @open="viewer = 'output'"
          />
          <div v-else-if="pdf" class="pages">
            <ImagePane
              v-for="(p, i) in pdf.output_pages"
              :key="i"
              side="output"
              :data="p"
              media-type="image/png"
              :page="i + 1"
              :findings="findings"
              :hovered="hovered"
              @hover="hovered = $event"
              @open="viewer = 'output'"
            />
          </div>
          <div v-else-if="result?.output_base64" class="binary">
            <strong>The redacted file is ready.</strong>
            <p v-if="result.media_type === 'application/pdf'">Every page rewritten as an image with its findings blacked out; the text layer, links and metadata are gone.</p>
            <p v-else>Every finding replaced in place, formatting kept; author and editor removed from its properties.</p>
            <button class="primary" @click="downloadOutput">Download {{ redactedName() }}</button>
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

    <ImageViewer
      v-if="viewer && (inputPictures || outputPictures)"
      :input="inputPictures"
      :output="outputPictures"
      :side="viewer"
      :findings="result ? findings : []"
      :hovered="hovered"
      @hover="hovered = $event"
      @close="viewer = null"
    />

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
.meta select {
  border: 1px solid var(--border-strong);
  background: var(--surface);
  color: var(--text);
  border-radius: var(--radius-sm);
  padding: 1px var(--space-1);
  font-size: 12.5px;
  cursor: pointer;
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
.binary {
  margin: var(--space-4);
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  align-items: flex-start;
  font-size: 13px;
  color: var(--text-2);
}
.failure {
  margin: var(--space-4);
  padding: var(--space-3) var(--space-4);
  border-radius: var(--radius);
  background: var(--danger-soft);
  color: var(--danger-text);
  font-size: 13px;
}
.failure .unsettled {
  margin-top: var(--space-2);
  color: var(--text-2);
}
.failure button {
  margin-top: var(--space-2);
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
