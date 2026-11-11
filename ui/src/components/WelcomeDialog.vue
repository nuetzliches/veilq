<script setup lang="ts">
import { ref } from 'vue';
import type { Api, Info } from '@/api';
import InstallCard from '@/components/InstallCard.vue';
import ModalDialog from '@/components/ModalDialog.vue';
import ModelsCard from '@/components/ModelsCard.vue';
import { t } from '@/lib/i18n';

// The first start (ADR-0033): install for this user, then the models that
// read pictures and names. Both stay in Settings for later.
defineProps<{ api: Api; info: Info }>();
const emit = defineEmits<{ close: []; changed: [] }>();

const ended = ref<string | null>(null);
</script>

<template>
  <ModalDialog :title="t('welcome.title')" wide @close="emit('close')">
    <div class="welcome">
      <p v-if="ended">{{ ended }}</p>
      <template v-else>
        <p class="intro">{{ t('welcome.intro') }}</p>
        <InstallCard :api="api" :info="info" compact @ended="ended = $event" />
        <ModelsCard :api="api" compact @changed="emit('changed')" />
        <footer>
          <span class="hint">{{ t('welcome.later') }}</span>
          <span class="spacer" />
          <button class="primary" @click="emit('close')">{{ t('welcome.start') }}</button>
        </footer>
      </template>
    </div>
  </ModalDialog>
</template>

<style scoped>
.welcome {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  padding: var(--space-4);
}
.intro {
  font-size: 14px;
  color: var(--text-2);
}
footer {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}
.hint {
  color: var(--text-muted);
  font-size: 12.5px;
}
.spacer {
  flex: 1;
}
</style>
