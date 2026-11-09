<script setup lang="ts">
import { onMounted, ref } from 'vue';
import type { Api, Listing } from '@/api';
import { formatSize } from '@/lib/segments';

const props = defineProps<{ api: Api; selected: string | null }>();
const emit = defineEmits<{ select: [path: string] }>();

const listing = ref<Listing | null>(null);
const error = ref<string | null>(null);
const loading = ref(false);

async function open(dir: string) {
  loading.value = true;
  error.value = null;
  try {
    listing.value = await props.api.files(dir);
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    loading.value = false;
  }
}

function crumbs(dir: string): { name: string; path: string }[] {
  const parts = dir ? dir.split('/') : [];
  return parts.map((name, i) => ({ name, path: parts.slice(0, i + 1).join('/') }));
}

function parent(dir: string): string {
  return dir.includes('/') ? dir.slice(0, dir.lastIndexOf('/')) : '';
}

onMounted(() => open(''));
</script>

<template>
  <div class="browser">
    <nav class="crumbs">
      <button class="link" @click="open('')">root</button>
      <template v-for="c in crumbs(listing?.dir ?? '')" :key="c.path">
        <span class="sep">/</span>
        <button class="link" @click="open(c.path)">{{ c.name }}</button>
      </template>
    </nav>
    <p v-if="error" class="error">{{ error }}</p>
    <ul v-else-if="listing" class="entries" :class="{ loading }">
      <li v-if="listing.dir">
        <button class="entry" @click="open(parent(listing.dir))"><span class="icon">↰</span>..</button>
      </li>
      <li v-for="e in listing.entries" :key="e.path">
        <button
          class="entry"
          :class="{ selected: e.path === selected }"
          @click="e.dir ? open(e.path) : emit('select', e.path)"
        >
          <span class="icon">{{ e.dir ? '▸' : '·' }}</span>
          <span class="name">{{ e.name }}</span>
          <span v-if="!e.dir" class="size">{{ formatSize(e.size) }}</span>
        </button>
      </li>
      <li v-if="!listing.entries.length" class="empty">empty</li>
      <li v-if="listing.truncated" class="empty">more entries than shown</li>
    </ul>
  </div>
</template>

<style scoped>
.browser {
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.crumbs {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 2px;
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--border);
  font-size: 12.5px;
}
.sep {
  color: var(--text-faint);
}
.entries {
  list-style: none;
  margin: 0;
  padding: var(--space-1);
  overflow: auto;
}
.entries.loading {
  opacity: 0.6;
}
.entry {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  width: 100%;
  border: 0;
  background: none;
  padding: 5px var(--space-2);
  border-radius: var(--radius-sm);
  text-align: left;
  cursor: pointer;
  font-size: 13px;
}
.entry:hover {
  background: var(--surface-hover);
}
.entry.selected {
  background: var(--accent-soft);
  color: var(--accent-text);
}
.icon {
  width: 1em;
  color: var(--text-muted);
}
.name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.size {
  color: var(--text-muted);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
}
.empty {
  color: var(--text-muted);
  padding: var(--space-2);
  font-size: 12.5px;
}
.error {
  color: var(--danger-text);
  padding: var(--space-3);
  font-size: 13px;
}
</style>
