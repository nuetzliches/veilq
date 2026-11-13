<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { ApiError, createApi, type Info, type Redaction, type Source } from '@/api';
import BrandMark from '@/components/BrandMark.vue';
import ComparePane from '@/components/ComparePane.vue';
import FileBrowser from '@/components/FileBrowser.vue';
import FindingsPanel from '@/components/FindingsPanel.vue';
import FolderRun from '@/components/FolderRun.vue';
import ImagePane from '@/components/ImagePane.vue';
import ImageViewer, { type Pictures } from '@/components/ImageViewer.vue';
import MissingModels from '@/components/MissingModels.vue';
import ModalDialog from '@/components/ModalDialog.vue';
import SettingsPage from '@/components/SettingsPage.vue';
import WelcomeDialog from '@/components/WelcomeDialog.vue';
import { copyablePicture, copyIsOurs, pngBlob } from '@/lib/clipboard';
import { locale, plural, t } from '@/lib/i18n';
import { actionLabel, missingModels } from '@/lib/labels';
import { pastedName } from '@/lib/paste';
import { change, percent } from '@/lib/threshold';
import {
  clampZoom,
  dragPan,
  findingIn,
  linkPanes,
  pageAnchor,
  pointIn,
  scrollFor,
  shownFindings,
  type ViewPoint,
  type Zoom,
  ZOOM_STEP,
} from '@/lib/view';

// One working surface (ADR-0033): the input on the left takes text, a
// dropped or pasted file and a screenshot alike; the redacted output on the
// right; the findings in a panel beside it. Opening a file, a folder run and
// the settings are behind the menu and the gear. Both sides are one view
// (ADR-0036): one zoom, one place, and a finding selected in the list, in a
// pane or with N and P shows on both.

const api = createApi();
const info = ref<Info | null>(null);
const infoError = ref<string | null>(null);

const text = ref('');
/** The input is the text editor; otherwise it shows the last run's source. */
const editing = ref(true);
const editor = ref<HTMLTextAreaElement | null>(null);
const picker = ref<HTMLInputElement | null>(null);
const dragging = ref(false);

const busy = ref(false);
const result = ref<Redaction | null>(null);
const error = ref<string | null>(null);
/** The last run failed because a picture's verify rounds did not settle: it can be written anyway. */
const unsettled = ref(false);
/** The source of the last run, shown above the input while it runs or failed. */
const sourceName = ref<string | null>(null);
const hovered = ref<number | null>(null);
/** The finding the panes show, picked in the list, in a pane, or with N and P. */
const selected = ref<number | null>(null);
const copied = ref(false);
/** The action of a run: `''` for the policy's, else one of `info.actions`. */
const action = ref('');
/** An image or a PDF's pages, shown as soon as chosen, before the redaction is back. */
const preview = ref<{ pages: string[]; mediaType: string; pageCount?: number } | null>(null);
/** The last run's source, run again when the action changes or a model arrives. */
const lastRun = ref<{ source: Source; name: string | null } | null>(null);
/**
 * The threshold of this source's runs (ADR-0046): `null` for the policy's. It saves nothing, and
 * another source starts at the policy's again.
 */
const threshold = ref<number | null>(null);
/** The slider's value while it is dragged; `null` when it rests. */
const dragged = ref<number | null>(null);
/** The findings panel shows what veilq read and why: asked for with its switch (ADR-0034). */
const explained = ref(false);

/** What is open over the surface. */
const settings = ref(false);
const welcome = ref(false);
const folderRun = ref(false);
const browsing = ref(false);
const menu = ref(false);
const panel = ref(false);

const findings = computed(() => result.value?.report.findings ?? []);
/** Where the threshold slider stands: dragged, set for this run, or the policy's default. */
const sliderValue = computed(() => dragged.value ?? threshold.value ?? info.value?.min_confidence ?? 0.7);
/** While the slider is dragged: what its value would change against the last run, from its explanation. */
const thresholdChange = computed(() => {
  const readings = result.value?.explain;
  return dragged.value === null || !readings ? null : change(readings, dragged.value);
});
/** Per finding, whether the panes can show it. */
const shown = computed(() => shownFindings(result.value));
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
/** The input shows something: a "New" button clears it. */
const filled = computed(() => !editing.value || !!text.value || !!error.value);
/** Models a failure or a review reason asks for. */
const wanted = computed(() =>
  missingModels([error.value ?? '', ...(result.value?.report.review_reasons ?? [])].join('\n')),
);

let heartbeat: ReturnType<typeof setInterval> | undefined;

watch(locale, (l) => (document.documentElement.lang = l), { immediate: true });

async function loadInfo() {
  try {
    info.value = await api.info();
    infoError.value = null;
  } catch (e) {
    infoError.value = e instanceof Error ? e.message : String(e);
  }
  // The models load beside the server: look again until they are there.
  if (info.value?.loading) setTimeout(() => void loadInfo(), 1000);
}

/** Whatever is open on top closes first; N and P step through the findings. */
function onKeyDown(event: KeyboardEvent) {
  const over = menu.value || browsing.value || folderRun.value || welcome.value || settings.value;
  const key = event.key.toLowerCase();
  if ((key === 'n' || key === 'p') && !over && findings.value.length) {
    const target = event.target as HTMLElement | null;
    if (event.ctrlKey || event.metaKey || event.altKey || target?.closest('textarea, input, select')) return;
    event.preventDefault();
    const n = findings.value.length;
    const at = selected.value ?? (key === 'n' ? -1 : n);
    void select((at + (key === 'n' ? 1 : n - 1)) % n);
    return;
  }
  if (event.key !== 'Escape' || viewer.value) return;
  if (menu.value) menu.value = false;
  else if (browsing.value) browsing.value = false;
  else if (folderRun.value) folderRun.value = false;
  else if (welcome.value) welcome.value = false;
  else if (settings.value) settings.value = false;
  else if (selected.value !== null) selected.value = null;
}

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

// One view over both sides (ADR-0036).
const inputBody = ref<HTMLElement | null>(null);
const outputBody = ref<HTMLElement | null>(null);
const zoom = ref<Zoom>('fit');
const linked = linkPanes(() => [inputBody.value, outputBody.value]);
/** The sides show pictures: the zoom scales them, else the text. */
const pictured = computed(() => !!(inputPictures.value || outputPictures.value));
/** The sides show something to zoom. */
const viewable = computed(() => !editing.value && (pictured.value || (!!result.value && !result.value.output_base64)));
const zoomLabel = computed(() =>
  zoom.value === 'fit' ? (pictured.value ? t('viewer.fit') : '100 %') : `${Math.round(zoom.value * 100)} %`,
);
/** A picture's zoom in a pane: without a scale it fits the pane's width. */
const paneZoom = computed(() => (zoom.value === 'fit' ? undefined : zoom.value));
/** A text's size. */
const textScale = computed(() => (zoom.value === 'fit' ? 1 : zoom.value));
/** Pictures are panned by dragging once they are zoomed. */
const pan = dragPan(() => pictured.value && zoom.value !== 'fit');

/** The pane that leads a change of the view: the input, unless it shows nothing to anchor. */
function leadPane(): HTMLElement | null {
  return [inputBody.value, outputBody.value].find((p) => p?.querySelector('[data-anchor]')) ?? null;
}

/** The scale the panes show now: in `fit`, a picture's as measured. */
function currentScale(): number {
  if (typeof zoom.value === 'number') return zoom.value;
  const img = pictured.value ? leadPane()?.querySelector('img') : null;
  return img && img.naturalWidth ? img.clientWidth / img.naturalWidth : 1;
}

/** Zooms both panes, keeping the point under `anchor` of pane `from` (else its middle) in place. */
async function zoomTo(next: Zoom, from: HTMLElement | null = leadPane(), anchor?: { x: number; y: number }) {
  const point = from && pointIn(from, anchor?.x, anchor?.y);
  zoom.value = next === 'fit' ? next : clampZoom(next, !pictured.value);
  await nextTick();
  if (!from || !point) return;
  const s = scrollFor(from, point, anchor?.x, anchor?.y);
  if (s) linked.scrollTo(from, s.left, s.top);
  linked.follow(from);
}

function onPaneWheel(event: WheelEvent) {
  if ((!event.ctrlKey && !event.metaKey) || !viewable.value) return;
  event.preventDefault();
  const el = event.currentTarget as HTMLElement;
  const r = el.getBoundingClientRect();
  const step = event.deltaY < 0 ? ZOOM_STEP : 1 / ZOOM_STEP;
  void zoomTo(currentScale() * step, el, { x: event.clientX - r.left, y: event.clientY - r.top });
}

/**
 * Selects finding `index` and shows it on both sides. Clicked in pane
 * `from`, that pane stays where it is and the other lines its finding up.
 */
async function select(index: number, from?: HTMLElement | null) {
  selected.value = index;
  panel.value = true;
  await nextTick();
  const panes = [inputBody.value, outputBody.value].filter((p): p is HTMLElement => !!p);
  const clicked = from && findingIn(from, index);
  // Both sides put it at one height: where it was clicked, else the input's middle.
  const at =
    from && clicked
      ? { x: clicked.x - from.scrollLeft, y: clicked.y - from.scrollTop }
      : panes[0] && { x: panes[0].clientWidth / 2, y: panes[0].clientHeight / 2 };
  for (const el of panes) {
    const p = findingIn(el, index);
    if (p && at && el !== from) linked.scrollTo(el, p.x - at.x, p.y - at.y);
  }
  // A side without the finding (no span there) follows a side with it.
  const lead = panes.find((el) => findingIn(el, index));
  if (lead && panes.some((el) => !findingIn(el, index))) linked.follow(lead);
}

/** A finding clicked in pane `side`. */
function selectIn(side: 'input' | 'output', index: number) {
  void select(index, side === 'input' ? inputBody.value : outputBody.value);
}

/** Where the viewer opens: the place the pane it was opened from shows. */
const viewerAt = ref<ViewPoint | null>(null);

function openViewer(side: 'input' | 'output') {
  const el = side === 'input' ? inputBody.value : outputBody.value;
  viewerAt.value = el && pointIn(el);
  viewer.value = side;
}

/** The viewer closed on `at`: the panes show that place. */
async function closeViewer(at: ViewPoint | null) {
  viewer.value = null;
  await nextTick();
  const el = leadPane();
  const s = el && at && scrollFor(el, at);
  if (!el || !s) return;
  linked.scrollTo(el, s.left, s.top);
  linked.follow(el);
}

async function run(source: Source, name: string | null, unverified = false, explain = false) {
  busy.value = true;
  viewer.value = null;
  error.value = null;
  unsettled.value = false;
  result.value = null;
  hovered.value = null;
  selected.value = null;
  // Another source starts at its own size and at the policy's threshold; the same one again keeps both.
  if (lastRun.value?.source !== source) {
    zoom.value = 'fit';
    threshold.value = null;
    explained.value = false;
  }
  if (explain) explained.value = true;
  sourceName.value = name;
  lastRun.value = { source, name };
  try {
    // The explanation comes with every run: the threshold slider counts what it would change from it (ADR-0046).
    result.value = await api.redact(source, {
      action: action.value || undefined,
      unverified,
      explain: true,
      minConfidence: threshold.value ?? undefined,
    });
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

/** What veilq read and why it decided (ADR-0034): the last run's, else the last source again. */
function runExplained() {
  if (result.value?.explain) explained.value = true;
  else if (lastRun.value && !busy.value) void run(lastRun.value.source, lastRun.value.name, false, true);
}

/** The slider moves: say what it would change, without a run (OCR takes seconds). */
function onThresholdInput(event: Event) {
  dragged.value = Number((event.target as HTMLInputElement).value);
}

/** The slider is let go: the last source again at its threshold; the policy's default is the policy as it is. */
function onThresholdChange(event: Event) {
  const value = Number((event.target as HTMLInputElement).value);
  dragged.value = null;
  const next = info.value && percent(value) === percent(info.value.min_confidence) ? null : value;
  if (next === threshold.value) return;
  threshold.value = next;
  runAgain();
}

/** The last source again: the action changed, or a model it needed arrived. */
function runAgain() {
  if (lastRun.value && !busy.value && !editing.value) void run(lastRun.value.source, lastRun.value.name);
}

function redactText() {
  if (!text.value.trim() || busy.value) return;
  editing.value = false;
  preview.value = null;
  void run({ kind: 'text', text: text.value }, null);
}

/** Back to the editor, with the text of the last run. */
async function editText() {
  if (busy.value) return;
  editing.value = true;
  result.value = null;
  error.value = null;
  await nextTick();
  editor.value?.focus();
}

/** An empty input again. */
async function clear() {
  if (busy.value) return;
  text.value = '';
  unsettled.value = false;
  preview.value = null;
  sourceName.value = null;
  lastRun.value = null;
  viewer.value = null;
  await editText();
}

const PICTURE = /\.(png|jpe?g|webp|gif)$/i;
const PDF = /\.pdf$/i;

/** A file under veilq's folder, opened from the file browser. */
function openFile(path: string) {
  browsing.value = false;
  settings.value = false;
  editing.value = false;
  preview.value = null;
  if (PICTURE.test(path) || PDF.test(path)) {
    api
      .picture(path)
      .then((p) => {
        if (lastRun.value?.name === path && !result.value) {
          preview.value = { pages: p.pages, mediaType: p.media_type, pageCount: p.page_count };
        }
      })
      .catch(() => {});
  }
  void run({ kind: 'file', path }, path);
}

async function upload(file: File | undefined) {
  if (!file || busy.value) return;
  editing.value = false;
  if (info.value && file.size > info.value.max_input) {
    result.value = null;
    preview.value = null;
    lastRun.value = null;
    sourceName.value = file.name;
    error.value = t('error.tooLarge', { name: file.name, size: info.value.max_input >> 20 });
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

/**
 * Ctrl+V with a file or an image in the clipboard (a screenshot) redacts it,
 * as a drop would. Text pasted into the editor stays plain pasting.
 */
function onPaste(event: ClipboardEvent) {
  const data = event.clipboardData;
  const target = event.target as HTMLElement | null;
  const editable = target?.closest('textarea, input, [contenteditable="true"]');
  if (!data || (editable && data.types.includes('text/plain'))) return;
  if (settings.value || welcome.value || folderRun.value || browsing.value) return;
  const file = Array.from(data.items)
    .find((i) => i.kind === 'file')
    ?.getAsFile();
  if (!file) {
    // Text pasted outside the editor starts a new text, unless one is being written.
    const pasted = data.getData('text/plain');
    if (!pasted || busy.value || (editing.value && text.value)) return;
    event.preventDefault();
    text.value = pasted;
    void editText();
    return;
  }
  event.preventDefault();
  void upload(new File([file], pastedName(file.name, file.type), { type: file.type }));
}

function chooseFile() {
  menu.value = false;
  picker.value?.click();
}

function onPick(event: Event) {
  const input = event.target as HTMLInputElement;
  void upload(input.files?.[0]);
  input.value = '';
}

/** What Ctrl+C or Copy puts on the clipboard: the redacted text, or a still picture as PNG. */
const copyable = computed(() => {
  const r = result.value;
  if (!r || error.value) return null;
  if (image.value && copyablePicture(image.value.media_type!)) return 'picture';
  return r.output_base64 ? null : 'text';
});

async function copyOutput() {
  const r = result.value;
  if (!r || !copyable.value) return;
  try {
    if (copyable.value === 'picture') {
      // A promise in the item keeps the click's permission while the PNG is made.
      await navigator.clipboard.write([new ClipboardItem({ 'image/png': pngBlob(r.output_base64!, r.media_type!) })]);
    } else {
      await navigator.clipboard.writeText(r.output);
    }
    copied.value = true;
    setTimeout(() => (copied.value = false), 1500);
  } catch (e) {
    error.value = t('error.copy', { message: e instanceof Error ? e.message : String(e) });
  }
}

/** Ctrl+C with nothing selected copies the output. */
function onCopy(event: ClipboardEvent) {
  if (!copyable.value || viewer.value || settings.value) return;
  if (!copyIsOurs(event.target, window.getSelection()?.toString() ?? '')) return;
  event.preventDefault();
  void copyOutput();
}

/** A file dragged anywhere over the window. */
function onDragOver(event: DragEvent) {
  if (!event.dataTransfer?.types.includes('Files')) return;
  if (settings.value || welcome.value || folderRun.value || browsing.value) return;
  event.preventDefault();
  dragging.value = true;
}

function onDrop(event: DragEvent) {
  if (!dragging.value) return;
  dragging.value = false;
  event.preventDefault();
  void upload(event.dataTransfer?.files[0]);
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
  window.addEventListener('copy', onCopy);
  window.addEventListener('keydown', onKeyDown);
  editor.value?.focus();
  await loadInfo();
  if (info.value?.desktop) {
    // A double click started veilq: it ends once this window has been gone a while.
    void api.ping().catch(() => {});
    heartbeat = setInterval(() => void api.ping().catch(() => {}), 20_000);
    // The first start welcomes, until veilq is installed.
    if (info.value.setup && !sessionStorage.getItem('veilq-setup-shown')) {
      sessionStorage.setItem('veilq-setup-shown', '1');
      const status = await api.setup().catch(() => null);
      if (status && !status.installed) welcome.value = true;
    }
  }
});

onBeforeUnmount(() => {
  window.removeEventListener('paste', onPaste);
  window.removeEventListener('copy', onCopy);
  window.removeEventListener('keydown', onKeyDown);
  clearInterval(heartbeat);
});
</script>

<template>
  <div class="app" :class="{ 'with-panel': panel && result }" @dragover="onDragOver" @drop="onDrop">
    <header class="topbar">
      <div class="brand">
        <BrandMark />
        <span>veilq</span>
        <span v-if="api.mock" class="badge warn" :title="t('top.mockTitle')">mock</span>
        <span v-if="info?.loading" class="badge warn" :title="t('top.loadingTitle')">{{ t('top.loading') }}</span>
        <span v-else-if="info?.load_error" class="badge danger" :title="info.load_error">{{ t('top.loadFailed') }}</span>
      </div>
      <span class="spacer" />
      <p v-if="infoError" class="top-error">{{ infoError }}</p>
      <div class="menu">
        <button :aria-expanded="menu" aria-haspopup="menu" @click="menu = !menu">{{ t('top.open') }} ▾</button>
        <div v-if="menu" class="menu-backdrop" @click="menu = false" />
        <ul v-if="menu" class="menu-list" role="menu">
          <li role="none">
            <button role="menuitem" @click="chooseFile">
              <strong>{{ t('menu.file') }}</strong><span>{{ t('menu.fileHint') }}</span>
            </button>
          </li>
          <li role="none">
            <button role="menuitem" :disabled="!info" @click="(menu = false), (folderRun = true)">
              <strong>{{ t('menu.folder') }}</strong><span>{{ t('menu.folderHint') }}</span>
            </button>
          </li>
        </ul>
      </div>
      <button
        class="icon"
        :title="t('top.settings')"
        :aria-label="t('top.settings')"
        :disabled="!info"
        @click="settings = true"
      >
        <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
          <path
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linejoin="round"
            d="M10.3 3.5h3.4l.5 2.4 1.6.9 2.3-.8 1.7 2.9-1.8 1.6v1.9l1.8 1.6-1.7 2.9-2.3-.8-1.6.9-.5 2.4h-3.4l-.5-2.4-1.6-.9-2.3.8-1.7-2.9 1.8-1.6V11L3.8 9.4l1.7-2.9 2.3.8 1.6-.9.9-2.9Z"
          />
          <circle cx="12" cy="12" r="3" fill="none" stroke="currentColor" stroke-width="1.8" />
        </svg>
      </button>
      <input ref="picker" type="file" hidden @change="onPick" />
    </header>

    <main class="compare">
      <section class="pane">
        <header class="pane-head">
          <h2>{{ t('input.title') }}</h2>
          <span v-if="result?.name ?? sourceName" class="mono name">{{ result?.name ?? sourceName }}</span>
          <span class="spacer" />
          <span v-if="viewable" class="zoom">
            <button class="small" :title="t('viewer.zoomOut')" @click="zoomTo(currentScale() / ZOOM_STEP)">−</button>
            <button class="small mono level" :title="t('pane.zoomLevelTitle')" @click="zoomTo('fit')">
              {{ zoomLabel }}
            </button>
            <button class="small" :title="t('viewer.zoomIn')" @click="zoomTo(currentScale() * ZOOM_STEP)">+</button>
          </span>
          <button v-if="inputPictures" class="small" :title="t('pane.zoomTitle')" @click="openViewer('input')">
            {{ t('pane.zoom') }}
          </button>
          <button v-if="filled" class="small" :disabled="busy" :title="t('input.newTitle')" @click="clear">
            {{ t('input.new') }}
          </button>
        </header>
        <div
          ref="inputBody"
          class="pane-body"
          :class="{ editing, pan: pictured && zoom !== 'fit' }"
          @scroll="linked.onScroll"
          @wheel="onPaneWheel"
          @pointerdown="pan.down"
          @pointermove="pan.move"
          @pointerup="pan.up"
          @pointercancel="pan.up"
        >
          <template v-if="editing">
            <textarea
              ref="editor"
              v-model="text"
              class="editor"
              spellcheck="false"
              :aria-label="t('input.editorLabel')"
              @keydown.ctrl.enter="redactText"
              @keydown.meta.enter="redactText"
            />
            <div v-if="!text" class="hint-overlay">
              <strong>{{ t('input.emptyTitle') }}</strong>
              <span>{{ t('input.emptyDrop') }}</span>
              <button class="link" @click="chooseFile">{{ t('menu.file') }}</button>
              <span class="faint">{{ t('input.emptyLocal') }}</span>
            </div>
            <footer v-if="text.trim()" class="pane-foot">
              <span class="faint">{{ t('input.redactHint') }}</span>
              <span class="spacer" />
              <button class="primary" :disabled="busy" @click="redactText">{{ t('input.redact') }}</button>
            </footer>
          </template>
          <ImagePane
            v-else-if="image"
            :anchor="pageAnchor(0)"
            side="input"
            :data="image.input_base64!"
            :media-type="image.media_type!"
            :findings="findings"
            :hovered="hovered"
            :selected="selected"
            @hover="hovered = $event"
            @select="selectIn('input', $event)"
            :zoom="paneZoom"
            @open="openViewer('input')"
          />
          <div v-else-if="pdf" class="pages">
            <ImagePane
              v-for="(p, i) in pdf.input_pages"
              :key="i"
              :anchor="pageAnchor(i)"
              side="input"
              :data="p"
              media-type="image/png"
              :page="i + 1"
              :findings="findings"
              :hovered="hovered"
              :selected="selected"
              @hover="hovered = $event"
              @select="selectIn('input', $event)"
              :zoom="paneZoom"
              @open="openViewer('input')"
            />
            <p v-if="(pdf.page_count ?? 0) > pdf.input_pages!.length" class="note">
              {{ t('pdf.morePages', { from: pdf.input_pages!.length + 1, to: pdf.page_count! }) }}
            </p>
          </div>
          <div v-else-if="result?.output_base64" class="binary">
            <strong>{{ result.name }}</strong>
            <p>{{ t('binary.input') }}</p>
          </div>
          <div
            v-else-if="lastRun?.source.kind === 'text'"
            class="editable"
            :title="t('input.editTitle')"
            @click="editText"
          >
            <ComparePane
              side="input"
              :text="result?.input ?? text"
              :spans="result?.input_spans ?? []"
              :findings="findings"
              :hovered="hovered"
              :selected="selected"
              :scale="textScale"
              @hover="hovered = $event"
              @select="selectIn('input', $event)"
            />
          </div>
          <ComparePane
            v-else-if="result"
            side="input"
            :text="result.input"
            :spans="result.input_spans"
            :findings="findings"
            :hovered="hovered"
            :selected="selected"
            :scale="textScale"
            @hover="hovered = $event"
            @select="selectIn('input', $event)"
          />
          <div v-else-if="preview" class="pages">
            <ImagePane
              v-for="(p, i) in preview.pages"
              :key="i"
              :anchor="pageAnchor(i)"
              side="input"
              :data="p"
              :media-type="preview.mediaType"
              :page="preview.pageCount ? i + 1 : undefined"
              :findings="[]"
              :hovered="null"
              :zoom="paneZoom"
              @open="openViewer('input')"
            />
          </div>
          <div v-else class="waiting">
            <strong v-if="busy">{{ t('input.reading') }}</strong>
          </div>
        </div>
      </section>

      <section class="pane">
        <header class="pane-head">
          <h2>{{ t('output.title') }}</h2>
          <label v-if="info && info.actions.length > 1" class="action">
            <span>{{ t('output.replaceWith') }}</span>
            <select v-model="action" :disabled="busy" @change="runAgain">
              <option value="">{{ t('output.asRules', { action: actionLabel(info.default_action) }) }}</option>
              <option v-for="a in info.actions" :key="a" :value="a">{{ actionLabel(a) }}</option>
            </select>
          </label>
          <label v-if="info && lastRun && !editing" class="threshold" :title="t('output.thresholdTitle')">
            <span>{{ t('output.threshold') }}</span>
            <input
              type="range"
              min="0"
              :max="info.max_min_confidence"
              step="0.01"
              :value="sliderValue"
              :disabled="busy"
              @input="onThresholdInput"
              @change="onThresholdChange"
            />
            <output :class="{ raised: percent(sliderValue) > percent(info.min_confidence) }">{{ percent(sliderValue) }} %</output>
            <span v-if="thresholdChange" class="change">
              {{ t('output.thresholdChange', { added: thresholdChange.added, dropped: thresholdChange.dropped }) }}
            </span>
          </label>
          <span class="spacer" />
          <button v-if="outputPictures" class="small" :title="t('pane.zoomTitle')" @click="openViewer('output')">
            {{ t('pane.zoom') }}
          </button>
          <button
            v-if="result"
            class="small"
            :class="{ on: panel }"
            :aria-pressed="panel"
            :title="t('output.findingsTitle')"
            @click="panel = !panel"
          >
            {{ plural(findings.length, 'output.findingsOne', 'output.findingsOther') }}
          </button>
        </header>
        <div
          ref="outputBody"
          class="pane-body"
          :class="{ pan: pictured && zoom !== 'fit' }"
          @scroll="linked.onScroll"
          @wheel="onPaneWheel"
          @pointerdown="pan.down"
          @pointermove="pan.move"
          @pointerup="pan.up"
          @pointercancel="pan.up"
        >
          <div v-if="result?.report.review_required" class="review" role="status">
            <strong>{{ t('output.review') }}</strong>
            <span v-for="(r, i) in result.report.review_reasons ?? []" :key="i">{{ r }}</span>
          </div>
          <div v-if="error" class="failure">
            <strong>{{ t('output.noOutput') }}</strong>
            <p>{{ error }}</p>
            <template v-if="unsettled">
              <p class="unsettled">{{ t('output.unsettled') }}</p>
              <button :disabled="busy" @click="runUnverified">{{ t('output.generateAnyway') }}</button>
            </template>
          </div>
          <MissingModels v-if="wanted.length && !busy" :api="api" :names="wanted" @ready="runAgain" />
          <template v-if="!error">
            <ImagePane
              v-if="image"
              :anchor="pageAnchor(0)"
              side="output"
              :data="image.output_base64!"
              :media-type="image.media_type!"
              :findings="findings"
              :hovered="hovered"
              :selected="selected"
              @hover="hovered = $event"
              @select="selectIn('output', $event)"
              :zoom="paneZoom"
              @open="openViewer('output')"
            />
            <div v-else-if="pdf" class="pages">
              <ImagePane
                v-for="(p, i) in pdf.output_pages"
                :key="i"
                :anchor="pageAnchor(i)"
                side="output"
                :data="p"
                media-type="image/png"
                :page="i + 1"
                :findings="findings"
                :hovered="hovered"
                :selected="selected"
                @hover="hovered = $event"
                @select="selectIn('output', $event)"
                :zoom="paneZoom"
                @open="openViewer('output')"
              />
            </div>
            <div v-else-if="result?.output_base64" class="binary">
              <strong>{{ t('binary.ready') }}</strong>
              <p v-if="result.media_type === 'application/pdf'">{{ t('binary.pdf') }}</p>
              <p v-else>{{ t('binary.office') }}</p>
            </div>
            <ComparePane
              v-else-if="result"
              side="output"
              :text="result.output"
              :spans="result.output_spans"
              :findings="findings"
              :hovered="hovered"
              :selected="selected"
              :scale="textScale"
              @hover="hovered = $event"
              @select="selectIn('output', $event)"
            />
            <div v-else class="waiting">
              <strong v-if="busy">{{ t('output.redacting') }}</strong>
              <span v-else class="faint">{{ t('output.empty') }}</span>
            </div>
          </template>
        </div>
        <footer v-if="result && !error" class="pane-foot">
          <span v-if="!findings.length" class="faint">{{ t('output.nothingFound') }}</span>
          <span class="spacer" />
          <button v-if="copyable" class="primary" :title="t('output.copyTitle')" @click="copyOutput">
            {{ copied ? t('output.copied') : t('output.copy') }}
          </button>
          <button :class="{ primary: !copyable }" @click="downloadOutput">{{ t('output.save') }}</button>
        </footer>
      </section>
    </main>

    <FindingsPanel
      v-if="panel && result"
      :findings="findings"
      :hovered="hovered"
      :selected="selected"
      :shown="shown"
      :readings="explained ? (result.explain ?? null) : null"
      :busy="busy"
      @hover="hovered = $event"
      @select="select"
      @close="panel = false"
      @explain="runExplained"
    />

    <div v-if="dragging" class="drop" @dragleave.self="dragging = false">
      <strong>{{ t('input.dropHere') }}</strong>
    </div>

    <ImageViewer
      v-if="viewer && (inputPictures || outputPictures)"
      :input="inputPictures"
      :output="outputPictures"
      :side="viewer"
      v-model:zoom="zoom"
      :findings="result ? findings : []"
      :hovered="hovered"
      :selected="selected"
      :at="viewerAt"
      @hover="hovered = $event"
      @select="select"
      @close="closeViewer"
    />

    <ModalDialog v-if="folderRun && info" :title="t('folder.title')" wide @close="folderRun = false">
      <FolderRun :api="api" :info="info" :action="action" @root="loadInfo" />
    </ModalDialog>

    <ModalDialog v-if="browsing && info" :title="t('files.title')" @close="browsing = false">
      <FileBrowser :api="api" :root="info.root" :selected="lastRun?.name ?? null" @select="openFile" />
    </ModalDialog>

    <SettingsPage
      v-if="settings && info"
      :api="api"
      :info="info"
      @close="settings = false"
      @changed="loadInfo"
      @browse="browsing = true"
    />

    <WelcomeDialog v-if="welcome && info" :api="api" :info="info" @close="welcome = false" @changed="loadInfo" />
  </div>
</template>

<style scoped>
.app {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  grid-template-rows: var(--topbar-h) minmax(0, 1fr);
  grid-template-areas:
    'top'
    'main';
  height: 100vh;
}
.app.with-panel {
  grid-template-columns: minmax(0, 1fr) 340px;
  grid-template-areas:
    'top top'
    'main panel';
}
.topbar {
  grid-area: top;
  display: flex;
  align-items: center;
  gap: var(--space-2);
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
.top-error {
  color: var(--danger-text);
  font-size: 12.5px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  min-width: 0;
}
button.icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 32px;
  padding: 0;
  color: var(--text-2);
}
.menu {
  position: relative;
}
.menu-backdrop {
  position: fixed;
  inset: 0;
  z-index: 5;
}
.menu-list {
  position: absolute;
  right: 0;
  top: calc(100% + 4px);
  z-index: 6;
  min-width: 300px;
  margin: 0;
  padding: var(--space-1);
  list-style: none;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--bg-elev);
  box-shadow: 0 8px 24px rgb(0 0 0 / 0.18);
}
.menu-list button {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  width: 100%;
  border: 0;
  background: none;
  text-align: left;
  padding: var(--space-2) var(--space-3);
}
.menu-list button span {
  color: var(--text-muted);
  font-size: 12.5px;
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
  container-type: inline-size;
}
.pane + .pane {
  border-left: 1px solid var(--border);
}
.pane-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  height: 44px;
  flex: none;
  padding: 0 var(--space-4);
  border-bottom: 1px solid var(--border);
  background: var(--surface-2);
}
.pane-head h2 {
  font-size: 14px;
  white-space: nowrap;
}
.pane-head button {
  white-space: nowrap;
  flex: none;
}
.pane-head button.on {
  background: var(--accent-soft);
  border-color: var(--accent);
  color: var(--accent-text);
}
.name {
  color: var(--text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.action {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  margin-left: var(--space-2);
  font-size: 12.5px;
  color: var(--text-muted);
  white-space: nowrap;
}
.action select {
  min-width: 5em;
  max-width: 15em;
  text-overflow: ellipsis;
}
/* The labels give way first: the slider's preview needs room while it is dragged. */
@container (max-width: 640px) {
  .action > span,
  .threshold > span:first-child {
    display: none;
  }
}
.threshold {
  display: flex;
  align-items: center;
  gap: 6px;
  /* Not shrunk: its preview would run under the findings button; the action's select gives way. */
  flex: none;
  margin-left: var(--space-2);
  font-size: 12.5px;
  color: var(--text-muted);
  white-space: nowrap;
}
.threshold input {
  width: 7em;
  min-width: 3em;
}
.threshold output {
  min-width: 3.2em;
  font-variant-numeric: tabular-nums;
  color: var(--text);
}
.threshold output.raised {
  color: var(--warn-text);
  font-weight: 600;
}
.threshold .change {
  font-variant-numeric: tabular-nums;
}
.action select {
  border: 1px solid var(--border-strong);
  background: var(--surface);
  color: var(--text);
  border-radius: var(--radius-sm);
  padding: 2px var(--space-1);
  font-size: 12.5px;
  cursor: pointer;
}
.spacer {
  flex: 1;
}
.pane-body {
  position: relative;
  flex: 1;
  overflow: auto;
  background: var(--surface);
}
.pane-body.pan {
  cursor: grab;
}
.pane-body.pan:active {
  cursor: grabbing;
}
.zoom {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  flex: none;
}
.zoom .level {
  min-width: 64px;
  color: var(--text-muted);
}
.pane-body.editing {
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.pane-foot {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex: none;
  min-height: 48px;
  padding: var(--space-2) var(--space-4);
  border-top: 1px solid var(--border);
  background: var(--surface-2);
}
.editor {
  flex: 1;
  width: 100%;
  border: 0;
  resize: none;
  padding: var(--space-4);
  background: transparent;
  font-family: var(--font-mono);
  font-size: 13px;
  line-height: 1.6;
}
.editor:focus {
  outline: none;
  box-shadow: none;
}
.hint-overlay {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  padding: var(--space-5);
  text-align: center;
  color: var(--text-muted);
  font-size: 13.5px;
  pointer-events: none;
}
.hint-overlay strong {
  color: var(--text-2);
  font-size: 17px;
  font-weight: 600;
}
.hint-overlay button {
  pointer-events: auto;
  font-size: 13.5px;
}
.faint {
  color: var(--text-faint);
  font-size: 12.5px;
}
.editable {
  min-height: 100%;
  cursor: text;
}
.waiting {
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-muted);
  font-size: 14px;
}
.note {
  color: var(--text-muted);
  font-size: 12.5px;
  padding: 0 var(--space-4) var(--space-4);
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
.review {
  position: sticky;
  top: 0;
  left: 0;
  z-index: 1;
  display: flex;
  flex-wrap: wrap;
  gap: 0 var(--space-2);
  padding: var(--space-2) var(--space-4);
  background: var(--warn-soft);
  color: var(--warn-text);
  font-size: 12.5px;
}
.drop {
  position: fixed;
  inset: 0;
  z-index: 30;
  display: flex;
  align-items: center;
  justify-content: center;
  margin: var(--space-3);
  border: 2px dashed var(--accent);
  border-radius: var(--radius);
  background: color-mix(in srgb, var(--accent-soft) 85%, transparent);
  color: var(--accent-text);
  font-size: 18px;
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
.badge.danger {
  background: var(--danger-soft);
  color: var(--danger-text);
}
@media (max-width: 900px) {
  .app,
  .app.with-panel {
    grid-template-columns: 1fr;
    grid-template-rows: var(--topbar-h) auto auto;
    grid-template-areas: 'top' 'main' 'panel';
    height: auto;
  }
  .compare {
    grid-template-columns: 1fr;
  }
  .pane {
    min-height: 50vh;
  }
  .pane + .pane {
    border-left: 0;
    border-top: 1px solid var(--border);
  }
}
</style>
