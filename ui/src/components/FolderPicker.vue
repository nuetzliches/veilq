<script setup lang="ts">
import { onMounted, ref } from 'vue';
import type { Api, Folders } from '@/api';
import { t } from '@/lib/i18n';

// Choosing a folder of this machine: a browser cannot name one, so the
// server lists folders (names only, no file is read) from the home folder,
// Documents, Desktop and the drives.
const props = defineProps<{ api: Api; title: string; start?: string; action?: string }>();
const emit = defineEmits<{ pick: [path: string]; close: [] }>();

const listing = ref<Folders | null>(null);
const error = ref<string | null>(null);
const typed = ref('');

async function open(path: string) {
  error.value = null;
  try {
    listing.value = await props.api.folders(path);
    typed.value = listing.value.path;
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  }
}

onMounted(() => open(props.start ?? ''));
</script>

<template>
  <div class="backdrop" @click.self="emit('close')">
    <div class="picker" role="dialog" :aria-label="title">
      <header>
        <h3>{{ title }}</h3>
        <span class="spacer" />
        <button class="small" @click="emit('close')">{{ t('common.cancel') }}</button>
      </header>
      <form class="path" @submit.prevent="open(typed)">
        <input v-model="typed" class="mono" :placeholder="t('picker.placeholder')" />
        <button class="small" type="submit">{{ t('picker.go') }}</button>
      </form>
      <p v-if="error" class="failure">{{ error }}</p>
      <ul v-if="listing" class="list">
        <li v-if="listing.path">
          <button class="link" @click="open(listing.parent ?? '')">↑ {{ listing.parent ? t('picker.up') : t('picker.start') }}</button>
        </li>
        <li v-for="f in listing.folders" :key="f.path">
          <button class="link" @click="open(f.path)">📁 {{ f.name }}</button>
        </li>
        <li v-if="!listing.folders.length" class="hint">{{ t('picker.none') }}</li>
      </ul>
      <footer>
        <span class="mono chosen">{{ listing?.path || t('picker.startingPoint') }}</span>
        <button class="primary" :disabled="!listing?.path" @click="emit('pick', listing!.path)">
          {{ action ?? t('picker.choose') }}
        </button>
      </footer>
    </div>
  </div>
</template>

<style scoped>
.backdrop {
  position: fixed;
  inset: 0;
  z-index: 20;
  display: flex;
  align-items: center;
  justify-content: center;
  background: color-mix(in srgb, var(--bg) 70%, transparent);
}
.picker {
  width: min(640px, 92vw);
  max-height: 80vh;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-4);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius);
  background: var(--surface);
}
header,
footer,
.path {
  display: flex;
  gap: var(--space-2);
  align-items: center;
}
header h3 {
  font-size: 14px;
}
.spacer {
  flex: 1;
}
.path input {
  flex: 1;
  padding: 4px var(--space-2);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: var(--surface-2);
}
.list {
  flex: 1;
  overflow: auto;
  margin: 0;
  padding: var(--space-1) 0;
  list-style: none;
  border-top: 1px solid var(--border);
  border-bottom: 1px solid var(--border);
  min-height: 200px;
}
.list li {
  padding: 2px 0;
}
.list .link {
  color: var(--text);
  font-size: 13px;
}
.chosen {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-muted);
}
.hint {
  color: var(--text-muted);
  font-size: 12.5px;
}
.failure {
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-sm);
  background: var(--danger-soft);
  color: var(--danger-text);
  font-size: 13px;
}
</style>
