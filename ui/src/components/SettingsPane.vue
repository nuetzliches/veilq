<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import type { Api, Info, ModelComponent, Models, SetupStatus, UpdateCheck } from '@/api';
import FolderPicker from '@/components/FolderPicker.vue';
import RulesCard from '@/components/RulesCard.vue';

// Settings (ADR-0027): setup for this user (install, Start menu, desktop,
// PATH), the models, and quitting a veilq a double click started.
const props = defineProps<{ api: Api; info: Info }>();
const emit = defineEmits<{ close: []; changed: [] }>();

const status = ref<SetupStatus | null>(null);
const wanted = ref({ start_menu: true, desktop: false, path: false });
const models = ref<Models | null>(null);
const error = ref<string | null>(null);
const busy = ref(false);
const done = ref<string | null>(null);
const removeModels = ref(false);
const ended = ref<string | null>(null);
const update = ref<UpdateCheck | null>(null);
const picking = ref(false);
const root = ref(props.info.root);

async function chooseRoot(path: string) {
  picking.value = false;
  error.value = null;
  try {
    root.value = (await props.api.setRoot(path)).root;
    emit('changed');
  } catch (e) {
    error.value = message(e);
  }
}
const checking = ref(false);
let poll: ReturnType<typeof setInterval> | undefined;

const running = computed(() => models.value?.components.some((c) => c.job?.state === 'running') ?? false);

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

async function loadSetup() {
  if (!props.info.setup) return;
  status.value = await props.api.setup();
  if (status.value.installed) {
    wanted.value = { start_menu: status.value.start_menu, desktop: status.value.desktop, path: status.value.on_path };
  }
}

async function loadModels() {
  const before = running.value;
  models.value = await props.api.models();
  if (before && !running.value) emit('changed');
}

async function install() {
  busy.value = true;
  error.value = null;
  done.value = null;
  try {
    status.value = await props.api.install(wanted.value);
    done.value = status.value.installed ? 'Installed for this user.' : 'Saved.';
  } catch (e) {
    error.value = message(e);
  } finally {
    busy.value = false;
  }
}

async function uninstall() {
  const what = removeModels.value ? 'veilq and its models' : 'veilq (the models stay)';
  if (!window.confirm(`Remove ${what} for this user? veilq ends, and this window stops working.`)) return;
  busy.value = true;
  error.value = null;
  try {
    await props.api.uninstall(removeModels.value);
    ended.value = 'veilq is removed. You can close this window.';
  } catch (e) {
    error.value = message(e);
  } finally {
    busy.value = false;
  }
}

async function fetchModel(c: ModelComponent) {
  error.value = null;
  try {
    await props.api.fetchModel(c.name);
    await loadModels();
  } catch (e) {
    error.value = message(e);
  }
}

async function checkUpdate() {
  checking.value = true;
  error.value = null;
  try {
    update.value = await props.api.checkUpdate();
  } catch (e) {
    error.value = message(e);
  } finally {
    checking.value = false;
  }
}

async function installUpdate() {
  if (!update.value) return;
  if (!window.confirm(`Install veilq ${update.value.latest} and restart it? This window stops working; veilq opens a new one.`)) return;
  checking.value = true;
  error.value = null;
  try {
    await props.api.installUpdate();
    ended.value = `veilq ${update.value.latest} is installed and opens in a new window. You can close this one.`;
  } catch (e) {
    error.value = message(e);
  } finally {
    checking.value = false;
  }
}

async function quit() {
  await props.api.quit().catch(() => {});
  ended.value = 'veilq has ended. You can close this window.';
}

function megabytes(bytes: number): string {
  return `${(bytes / 2 ** 20).toFixed(1)} MB`;
}

function state(c: ModelComponent): string {
  if (!c.available) return 'not built for this platform';
  if (c.active) return 'active';
  if (c.installed) return 'installed, not active: restart veilq';
  return 'not installed';
}

onMounted(async () => {
  try {
    await Promise.all([loadSetup(), loadModels()]);
  } catch (e) {
    error.value = message(e);
  }
  poll = setInterval(() => {
    if (running.value) void loadModels().catch(() => {});
  }, 1000);
});
onBeforeUnmount(() => clearInterval(poll));
</script>

<template>
  <div class="settings" role="dialog" aria-label="Settings">
    <header class="settings-head">
      <h2>Settings</h2>
      <span class="spacer" />
      <button class="small" @click="emit('close')">Close <kbd>Esc</kbd></button>
    </header>
    <div class="settings-body">
      <p v-if="ended" class="ended">{{ ended }}</p>
      <template v-else>
        <p v-if="error" class="failure">{{ error }}</p>

        <section v-if="info.setup && status" class="card">
          <h3>Setup</h3>
          <p class="hint">
            Puts veilq into <span class="mono">{{ status.install_dir }}</span> for you alone; no administrator rights.
            <template v-if="status.runs_installed">It runs from there.</template>
            <template v-else>It runs from <span class="mono">{{ status.exe }}</span> now.</template>
          </p>
          <label><input v-model="wanted.start_menu" type="checkbox" /> Start menu entry</label>
          <label><input v-model="wanted.desktop" type="checkbox" /> Desktop shortcut</label>
          <label>
            <input v-model="wanted.path" type="checkbox" /> Command line: <span class="mono">veilq</span> in new terminals
          </label>
          <p class="hint">
            To pin veilq to the taskbar, right-click its Start menu entry and choose "Pin to taskbar": Windows lets
            no program do that itself.
          </p>
          <div class="row">
            <button class="primary" :disabled="busy" @click="install">
              {{ status.installed ? 'Apply' : 'Install' }}
            </button>
            <span v-if="done" class="ok">{{ done }}</span>
          </div>
          <details v-if="status.installed" class="remove">
            <summary>Uninstall</summary>
            <label><input v-model="removeModels" type="checkbox" /> Remove the downloaded models too</label>
            <button :disabled="busy" @click="uninstall">Uninstall veilq</button>
          </details>
        </section>

        <section class="card">
          <h3>Folder</h3>
          <p class="hint">
            The File tab opens files under this folder, and the Folder tab redacts folders in it.
            <template v-if="info.desktop">veilq keeps it for its next start.</template>
          </p>
          <div class="row">
            <span class="mono">{{ root }}</span>
            <button class="small" @click="picking = true">Choose …</button>
          </div>
        </section>
        <FolderPicker
          v-if="picking"
          :api="api"
          title="The folder veilq reads"
          :start="root"
          @pick="chooseRoot"
          @close="picking = false"
        />

        <RulesCard :api="api" @saved="emit('changed')" />

        <section class="card">
          <h3>Models</h3>
          <p class="hint">
            veilq finds most data with patterns. These add more; each is downloaded once from its publisher and checked
            against a checksum built into veilq. Nothing else is sent anywhere.
            <span v-if="models" class="mono">{{ models.dir }}</span>
          </p>
          <div v-for="c in models?.components ?? []" :key="c.name" class="model">
            <div class="model-head">
              <strong class="mono">{{ c.name }}</strong>
              <span class="badge" :class="{ on: c.active }">{{ state(c) }}</span>
              <span class="spacer" />
              <button
                v-if="c.available && !c.installed && c.job?.state !== 'running'"
                class="small"
                @click="fetchModel(c)"
              >
                Download
              </button>
            </div>
            <p class="about">{{ c.about }}</p>
            <p v-if="c.job?.state === 'running'" class="progress">
              Downloading {{ c.job.file }} ({{ c.job.index }}/{{ c.job.of }}): {{ megabytes(c.job.bytes) }}
            </p>
            <p v-else-if="c.job?.state === 'failed'" class="failure">{{ c.job.error }}</p>
          </div>
        </section>

        <section class="card">
          <h3>Updates</h3>
          <p class="hint">
            veilq {{ info.version }}. Checking asks GitHub for the newest release and sends nothing else; an update is
            checked against the release's checksums before it replaces this program.
          </p>
          <div class="row">
            <button :disabled="checking" @click="checkUpdate">{{ checking ? 'Checking …' : 'Check for updates' }}</button>
            <template v-if="update">
              <span v-if="!update.newer" class="ok">veilq {{ update.current }} is the newest release.</span>
              <span v-else>
                veilq {{ update.latest }} is out.
                <a :href="update.url" target="_blank" rel="noopener">What changed</a>
              </span>
            </template>
          </div>
          <template v-if="update?.newer">
            <button v-if="info.desktop && update.archive" class="primary" :disabled="checking" @click="installUpdate">
              Install {{ update.latest }} and restart
            </button>
            <p v-else-if="!info.desktop" class="hint">
              This veilq runs from a command line: update it there with <span class="mono">veilq update</span>.
            </p>
          </template>
        </section>

        <section v-if="info.desktop" class="card">
          <h3>veilq</h3>
          <p class="hint">
            veilq runs in the background while this window is open, and ends a few minutes after it is closed.
          </p>
          <button @click="quit">Quit veilq</button>
        </section>
      </template>
    </div>
  </div>
</template>

<style scoped>
.settings {
  position: fixed;
  inset: 0;
  z-index: 10;
  display: flex;
  flex-direction: column;
  background: var(--bg);
}
.settings-head {
  display: flex;
  align-items: center;
  height: 48px;
  padding: 0 var(--space-4);
  border-bottom: 1px solid var(--border);
  background: var(--bg-elev);
}
.settings-head h2 {
  font-size: 15px;
}
.spacer {
  flex: 1;
}
.settings-body {
  flex: 1;
  overflow: auto;
  padding: var(--space-5);
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  max-width: 760px;
  width: 100%;
  margin: 0 auto;
}
.card {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-4);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--surface);
}
.card h3 {
  font-size: 14px;
}
.card label {
  display: flex;
  gap: var(--space-2);
  align-items: center;
  font-size: 13px;
}
.card > button {
  align-self: flex-start;
}
.hint,
.about {
  color: var(--text-muted);
  font-size: 12.5px;
}
.row {
  display: flex;
  gap: var(--space-3);
  align-items: center;
}
.ok {
  color: var(--ok-text);
  font-size: 13px;
}
.remove {
  margin-top: var(--space-2);
  font-size: 13px;
}
.remove summary {
  cursor: pointer;
  color: var(--text-2);
}
.remove label,
.remove button {
  margin-top: var(--space-2);
}
.model {
  padding: var(--space-2) 0;
  border-top: 1px solid var(--border);
}
.model-head {
  display: flex;
  gap: var(--space-2);
  align-items: center;
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
.failure {
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-sm);
  background: var(--danger-soft);
  color: var(--danger-text);
  font-size: 13px;
}
.ended {
  font-size: 14px;
}
kbd {
  font-size: 11px;
  opacity: 0.6;
}
</style>
