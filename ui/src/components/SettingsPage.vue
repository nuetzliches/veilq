<script setup lang="ts">
import { ref } from 'vue';
import type { Api, Info } from '@/api';
import FolderPicker from '@/components/FolderPicker.vue';
import InstallCard from '@/components/InstallCard.vue';
import ModelsCard from '@/components/ModelsCard.vue';
import RulesCard from '@/components/RulesCard.vue';
import UpdatesCard from '@/components/UpdatesCard.vue';
import { choice, setChoice, t, type Choice, type MessageKey } from '@/lib/i18n';
import { actionLabel } from '@/lib/labels';

// The settings (ADR-0033), apart from the first start: what a user changes
// now and then under General, what veilq finds under Detection, and what
// describes this installation under Advanced. A section for an
// organisation's server (#34, #35) and for receipts (#37) goes beside these.
const props = defineProps<{ api: Api; info: Info }>();
const emit = defineEmits<{ close: []; changed: []; browse: [] }>();

type Section = 'general' | 'detection' | 'advanced';
const sections: { id: Section; label: MessageKey }[] = [
  { id: 'general', label: 'settings.general' },
  { id: 'detection', label: 'settings.detection' },
  { id: 'advanced', label: 'settings.advanced' },
];

const section = ref<Section>('general');
const ended = ref<string | null>(null);
const error = ref<string | null>(null);
const picking = ref(false);

const languages: { value: Choice; label: MessageKey }[] = [
  { value: 'system', label: 'settings.languageSystem' },
  { value: 'de', label: 'settings.languageDe' },
  { value: 'en', label: 'settings.languageEn' },
];

async function chooseRoot(path: string) {
  picking.value = false;
  error.value = null;
  try {
    await props.api.setRoot(path);
    emit('changed');
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  }
}

async function quit() {
  await props.api.quit().catch(() => {});
  ended.value = t('settings.quitDone');
}
</script>

<template>
  <div class="settings" role="dialog" :aria-label="t('settings.title')">
    <header class="settings-head">
      <button class="small" @click="emit('close')">← {{ t('settings.back') }}</button>
      <h2>{{ t('settings.title') }}</h2>
    </header>
    <p v-if="ended" class="ended">{{ ended }}</p>
    <div v-else class="settings-main">
      <nav class="settings-nav">
        <button
          v-for="s in sections"
          :key="s.id"
          :class="{ active: section === s.id }"
          :aria-current="section === s.id ? 'page' : undefined"
          @click="section = s.id"
        >
          {{ t(s.label) }}
        </button>
      </nav>
      <div class="settings-body">
        <p v-if="error" class="failure">{{ error }}</p>

        <template v-if="section === 'general'">
          <section class="card">
            <h3>{{ t('settings.language') }}</h3>
            <div class="row">
              <label v-for="l in languages" :key="l.value" class="choice">
                <input type="radio" name="language" :checked="choice === l.value" @change="setChoice(l.value)" />
                {{ t(l.label) }}
              </label>
            </div>
          </section>
          <UpdatesCard :api="api" :info="info" @ended="ended = $event" />
          <section v-if="info.desktop" class="card">
            <h3>{{ t('settings.quitTitle') }}</h3>
            <p class="hint">{{ info.window ? t('settings.quitWindow') : t('settings.quitBackground') }}</p>
            <button @click="quit">{{ t('settings.quit') }}</button>
          </section>
        </template>

        <template v-else-if="section === 'detection'">
          <RulesCard :api="api" @saved="emit('changed')" />
          <ModelsCard :api="api" @changed="emit('changed')" />
        </template>

        <template v-else>
          <section class="card">
            <h3>{{ t('settings.folderTitle') }}</h3>
            <p class="hint">{{ t('settings.folderAbout') }}</p>
            <div class="row">
              <span class="mono path">{{ info.root }}</span>
              <button class="small" @click="picking = true">{{ t('common.choose') }}</button>
              <button class="small" @click="emit('browse')">{{ t('settings.browse') }}</button>
            </div>
          </section>
          <InstallCard v-if="info.setup" :api="api" :info="info" @ended="ended = $event" />
          <section class="card">
            <h3>{{ t('settings.aboutTitle') }}</h3>
            <dl class="facts">
              <dt>{{ t('settings.version') }}</dt>
              <dd class="mono">{{ info.version }}</dd>
              <dt>{{ t('settings.policy') }}</dt>
              <dd class="mono">{{ info.policy }}</dd>
              <dt>{{ t('settings.defaultAction') }}</dt>
              <dd>{{ actionLabel(info.default_action) }}</dd>
            </dl>
            <p class="hint">{{ t('settings.cli') }}</p>
          </section>
        </template>
      </div>
    </div>
    <FolderPicker
      v-if="picking"
      :api="api"
      :title="t('settings.folderTitle')"
      :start="info.root"
      @pick="chooseRoot"
      @close="picking = false"
    />
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
  gap: var(--space-3);
  height: var(--topbar-h);
  flex: none;
  padding: 0 var(--space-4);
  border-bottom: 1px solid var(--border);
  background: var(--bg-elev);
}
.settings-head h2 {
  font-size: 16px;
}
.settings-main {
  flex: 1;
  min-height: 0;
  display: flex;
  justify-content: center;
  gap: var(--space-5);
  padding: var(--space-5);
  overflow: auto;
}
.settings-nav {
  display: flex;
  flex-direction: column;
  gap: 2px;
  width: 180px;
  flex: none;
}
.settings-nav button {
  border: 0;
  background: none;
  text-align: left;
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-sm);
  color: var(--text-2);
}
.settings-nav button.active {
  background: var(--accent-soft);
  color: var(--accent-text);
  font-weight: 500;
}
.settings-body {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  width: 100%;
  max-width: 720px;
}
.choice {
  display: flex;
  gap: 6px;
  align-items: center;
}
.path {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.facts {
  display: grid;
  grid-template-columns: max-content 1fr;
  gap: 4px var(--space-4);
  margin: 0;
}
.facts dt {
  color: var(--text-muted);
}
.facts dd {
  margin: 0;
}
.failure {
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-sm);
  background: var(--danger-soft);
  color: var(--danger-text);
  font-size: 13px;
}
.ended {
  padding: var(--space-5);
  font-size: 14px;
}
@media (max-width: 700px) {
  .settings-main {
    flex-direction: column;
  }
  .settings-nav {
    flex-direction: row;
    width: auto;
  }
}
</style>
