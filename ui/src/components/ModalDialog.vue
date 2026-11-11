<script setup lang="ts">
import { t } from '@/lib/i18n';

// A dialog over the working surface: a title, a close button, the content.
defineProps<{ title: string; wide?: boolean }>();
const emit = defineEmits<{ close: [] }>();
</script>

<template>
  <div class="backdrop" @click.self="emit('close')">
    <div class="dialog" :class="{ wide }" role="dialog" :aria-label="title">
      <header>
        <h2>{{ title }}</h2>
        <span class="spacer" />
        <button class="small" @click="emit('close')">{{ t('common.close') }}</button>
      </header>
      <div class="body">
        <slot />
      </div>
    </div>
  </div>
</template>

<style scoped>
.backdrop {
  position: fixed;
  inset: 0;
  z-index: 15;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--space-4);
  background: color-mix(in srgb, var(--bg) 70%, transparent);
}
.dialog {
  display: flex;
  flex-direction: column;
  width: min(560px, 100%);
  max-height: min(720px, 100%);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--bg-elev);
  box-shadow: 0 16px 48px rgb(0 0 0 / 0.25);
}
.dialog.wide {
  width: min(820px, 100%);
}
header {
  display: flex;
  align-items: center;
  padding: var(--space-3) var(--space-4);
  border-bottom: 1px solid var(--border);
}
h2 {
  font-size: 15px;
}
.spacer {
  flex: 1;
}
.body {
  flex: 1;
  min-height: 0;
  overflow: auto;
}
</style>
