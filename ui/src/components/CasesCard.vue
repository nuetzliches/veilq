<script setup lang="ts">
import { onMounted, ref } from 'vue';
import type { Api, CaseCount } from '@/api';
import { t } from '@/lib/i18n';

// The cases a user's marks made (#34): kept on this computer only, deleted
// here at any time.
const props = defineProps<{ api: Api }>();

const cases = ref<CaseCount | null>(null);
const busy = ref(false);
const error = ref<string | null>(null);
const note = ref<string | null>(null);

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

async function load() {
  try {
    cases.value = await props.api.cases();
  } catch (e) {
    error.value = message(e);
  }
}

async function deleteAll() {
  if (!window.confirm(t('cases.confirm'))) return;
  busy.value = true;
  error.value = null;
  try {
    const { deleted } = await props.api.deleteCases();
    note.value = t('cases.deleted', { n: deleted });
    await load();
  } catch (e) {
    error.value = message(e);
  } finally {
    busy.value = false;
  }
}

onMounted(load);
</script>

<template>
  <section class="card">
    <h3>{{ t('cases.title') }}</h3>
    <p class="hint">{{ t('cases.text') }}</p>
    <p v-if="error" class="failure">{{ error }}</p>
    <template v-if="cases">
      <p v-if="!cases.key_store" class="hint">{{ t('cases.noKeyStore') }}</p>
      <div class="row">
        <span>{{ t('cases.count', { n: cases.count }) }}</span>
        <button :disabled="busy || !cases.count" @click="deleteAll">{{ t('cases.delete') }}</button>
        <span v-if="note" class="ok">{{ note }}</span>
      </div>
    </template>
  </section>
</template>
