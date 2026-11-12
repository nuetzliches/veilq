<script setup lang="ts">
import { ref } from 'vue';
import type { Api, Info, UpdateCheck } from '@/api';
import { t } from '@/lib/i18n';

// The update check (ADR-0027): only when asked; an update is checked
// against the release's checksums before it replaces this program.
const props = defineProps<{ api: Api; info: Info }>();
const emit = defineEmits<{ ended: [message: string] }>();

const update = ref<UpdateCheck | null>(null);
const checking = ref(false);
const error = ref<string | null>(null);

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

async function check() {
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

async function install() {
  if (!update.value) return;
  if (!window.confirm(t('updates.confirm', { version: update.value.latest }))) return;
  checking.value = true;
  error.value = null;
  try {
    await props.api.installUpdate();
    emit('ended', t('updates.installed', { version: update.value.latest }));
  } catch (e) {
    error.value = message(e);
  } finally {
    checking.value = false;
  }
}
</script>

<template>
  <section class="card">
    <h3>{{ t('updates.title') }}</h3>
    <!-- Installed from the Microsoft Store: the Store updates veilq (ADR-0039). -->
    <p v-if="info.store" class="hint">{{ t('updates.store', { version: info.version }) }}</p>
    <template v-else>
      <p class="hint">{{ t('updates.about', { version: info.version }) }}</p>
      <p v-if="error" class="failure">{{ error }}</p>
      <div class="row">
        <button :disabled="checking" @click="check">{{ checking ? t('updates.checking') : t('updates.check') }}</button>
        <template v-if="update">
          <span v-if="!update.newer" class="ok">{{ t('updates.newest', { version: update.current }) }}</span>
          <span v-else>
            {{ t('updates.out', { version: update.latest }) }}
            <a :href="update.url" target="_blank" rel="noopener">{{ t('updates.changes') }}</a>
          </span>
        </template>
      </div>
      <template v-if="update?.newer">
        <button v-if="info.desktop && update.archive" class="primary" :disabled="checking" @click="install">
          {{ t('updates.install', { version: update.latest }) }}
        </button>
        <p v-else-if="!info.desktop" class="hint">{{ t('updates.cli') }}</p>
      </template>
    </template>
  </section>
</template>
