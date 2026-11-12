<script setup lang="ts">
import { onMounted, ref } from 'vue';
import type { Api, Info, PolicyView } from '@/api';
import InstallCard from '@/components/InstallCard.vue';
import ModalDialog from '@/components/ModalDialog.vue';
import ModelsCard from '@/components/ModelsCard.vue';
import PurposesPicker from '@/components/PurposesPicker.vue';
import { t } from '@/lib/i18n';

// The first start (ADR-0033): what veilq is used for (ADR-0040), install
// for this user, then the models that read pictures and names. All stay in
// Settings for later. A purpose chosen here is saved to the user's policy
// at once.
const props = defineProps<{ api: Api; info: Info }>();
const emit = defineEmits<{ close: []; changed: [] }>();

const ended = ref<string | null>(null);
const view = ref<PolicyView | null>(null);
const purposes = ref<string[]>(['general']);
const purposeError = ref<string | null>(null);

onMounted(async () => {
  try {
    view.value = await props.api.policy();
    purposes.value = view.value.policy.purposes ?? ['general'];
  } catch {
    view.value = null;
  }
});

async function choose(next: string[]) {
  const before = purposes.value;
  purposes.value = next;
  purposeError.value = null;
  try {
    await props.api.savePolicy({ ...view.value!.policy, purposes: next });
    view.value = await props.api.policy();
    emit('changed');
  } catch (e) {
    purposes.value = before;
    purposeError.value = e instanceof Error ? e.message : String(e);
  }
}
</script>

<template>
  <ModalDialog :title="t('welcome.title')" wide @close="emit('close')">
    <div class="welcome">
      <p v-if="ended">{{ ended }}</p>
      <template v-else>
        <p class="intro">{{ t('welcome.intro') }}</p>
        <section v-if="view">
          <h4>{{ t('welcome.purposes') }}</h4>
          <PurposesPicker :model-value="purposes" :purposes="view.purposes" @update:model-value="choose" />
          <p v-if="purposeError" class="failure">{{ purposeError }}</p>
        </section>
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
