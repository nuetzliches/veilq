<script setup lang="ts">
import { onMounted, ref } from 'vue';
import type { Api, Info, SetupStatus } from '@/api';
import { t } from '@/lib/i18n';

// Setup for this user (ADR-0027): the program folder, the Start menu, a
// desktop shortcut, PATH; uninstall. `compact` for the welcome dialog: no
// uninstall, no taskbar hint.
const props = defineProps<{ api: Api; info: Info; compact?: boolean }>();
const emit = defineEmits<{ installed: []; ended: [message: string] }>();

const status = ref<SetupStatus | null>(null);
const wanted = ref({ start_menu: true, desktop: false, path: false });
const error = ref<string | null>(null);
const busy = ref(false);
const done = ref<string | null>(null);
const removeModels = ref(false);

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

async function install() {
  busy.value = true;
  error.value = null;
  done.value = null;
  try {
    status.value = await props.api.install(wanted.value);
    done.value = status.value.installed ? t('install.done') : t('install.saved');
    if (status.value.installed) emit('installed');
  } catch (e) {
    error.value = message(e);
  } finally {
    busy.value = false;
  }
}

async function uninstall() {
  const what = removeModels.value ? t('install.confirmWithModels') : t('install.confirmWithoutModels');
  if (!window.confirm(what)) return;
  busy.value = true;
  error.value = null;
  try {
    await props.api.uninstall(removeModels.value);
    emit('ended', t('install.removed'));
  } catch (e) {
    error.value = message(e);
  } finally {
    busy.value = false;
  }
}

onMounted(async () => {
  try {
    status.value = await props.api.setup();
    if (status.value.installed) {
      wanted.value = { start_menu: status.value.start_menu, desktop: status.value.desktop, path: status.value.on_path };
    }
  } catch (e) {
    error.value = message(e);
  }
});
</script>

<template>
  <section class="card">
    <h3>{{ t('install.title') }}</h3>
    <p v-if="error" class="failure">{{ error }}</p>
    <template v-if="status">
      <p class="hint">{{ t('install.about') }}</p>
      <label><input v-model="wanted.start_menu" type="checkbox" /> {{ t('install.startMenu') }}</label>
      <label><input v-model="wanted.desktop" type="checkbox" /> {{ t('install.desktop') }}</label>
      <label v-if="!compact"><input v-model="wanted.path" type="checkbox" /> {{ t('install.path') }}</label>
      <p v-if="!compact" class="hint">{{ info.window ? t('install.pinWindow') : t('install.pinStart') }}</p>
      <div class="row">
        <button class="primary" :disabled="busy" @click="install">
          {{ status.installed ? t('install.apply') : t('install.install') }}
        </button>
        <span v-if="done" class="ok">{{ done }}</span>
      </div>
      <p v-if="!compact" class="hint mono">{{ status.install_dir }}</p>
      <details v-if="status.installed && !compact" class="remove">
        <summary>{{ t('install.uninstall') }}</summary>
        <label><input v-model="removeModels" type="checkbox" /> {{ t('install.removeModels') }}</label>
        <button :disabled="busy" @click="uninstall">{{ t('install.uninstallButton') }}</button>
      </details>
    </template>
  </section>
</template>

<style scoped>
.card > label {
  display: flex;
  gap: var(--space-2);
  align-items: center;
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
</style>
