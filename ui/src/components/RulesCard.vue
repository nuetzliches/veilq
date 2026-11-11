<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import type { Api, Policy, PolicyTried, PolicyView } from '@/api';

// The user's policy as a form (ADR-0028): what veilq replaces and how,
// what is never personal data, and the user's own patterns, tried on a
// sample before it is saved. Saved to the user's veilq.toml; a project's
// veilq.toml comes first where there is one.
const props = defineProps<{ api: Api }>();
const emit = defineEmits<{ saved: [] }>();

const view = ref<PolicyView | null>(null);
const policy = ref<Policy | null>(null);
const allowValues = ref('');
const allowPatterns = ref('');
const sample = ref('Rückruf an Mira Albers, Kundennummer KD-482913, Tel. 0421 5550173.');
const tried = ref<PolicyTried | null>(null);
const error = ref<string | null>(null);
const saved = ref<string | null>(null);
const busy = ref(false);

const entityRows = computed(() => view.value?.entities ?? []);

/** A plain copy: the policy is JSON, and a reactive proxy cannot be `structuredClone`d. */
function clone<T>(v: T): T {
  return JSON.parse(JSON.stringify(v)) as T;
}

function lines(s: string): string[] {
  return s
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean);
}

/** The form as a policy: empty entity rules left out. */
function current(): Policy {
  const p = clone(policy.value!);
  p.allow = { values: lines(allowValues.value), patterns: lines(allowPatterns.value) };
  for (const [k, v] of Object.entries(p.entities)) {
    if (!v.action) delete v.action;
    if (v.min_confidence === undefined || (v.min_confidence as unknown) === '') delete v.min_confidence;
    if (!v.action && v.min_confidence === undefined) delete p.entities[k];
  }
  p.patterns = p.patterns.map((r) => ({ ...r, confidence: r.confidence || undefined }));
  return p;
}

function entityRule(entity: string) {
  return policy.value!.entities[entity]!;
}

function addPattern() {
  policy.value!.patterns.push({ name: `pattern-${policy.value!.patterns.length + 1}`, entity: 'CUSTOMER_NUMBER', regex: '' });
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

async function tryIt() {
  error.value = null;
  saved.value = null;
  try {
    tried.value = await props.api.tryPolicy(current(), sample.value);
  } catch (e) {
    tried.value = null;
    error.value = message(e);
  }
}

async function save() {
  busy.value = true;
  error.value = null;
  saved.value = null;
  try {
    const r = await props.api.savePolicy(current());
    saved.value = r.in_use
      ? `Saved to ${r.path}; veilq uses it now.`
      : `Saved to ${r.path}. A project's veilq.toml is in use here and comes first.`;
    emit('saved');
    view.value = await props.api.policy();
  } catch (e) {
    error.value = message(e);
  } finally {
    busy.value = false;
  }
}

onMounted(async () => {
  try {
    view.value = await props.api.policy();
    const p = clone(view.value.policy);
    // Every type gets a row; empty ones are left out again when saved.
    for (const e of view.value.entities) p.entities[e] ??= {};
    policy.value = p;
    allowValues.value = view.value.policy.allow.values.join('\n');
    allowPatterns.value = view.value.policy.allow.patterns.join('\n');
  } catch (e) {
    error.value = message(e);
  }
});
</script>

<template>
  <section class="card">
    <h3>Rules</h3>
    <p v-if="view" class="hint">
      In use: <span class="mono">{{ view.source }}</span>.
      <template v-if="view.user_in_use">Your rules are saved to <span class="mono">{{ view.user_file }}</span>.</template>
      <template v-else>
        Your rules (<span class="mono">{{ view.user_file }}</span>) apply where no project has a veilq.toml; here one
        does.
      </template>
    </p>
    <p v-if="error" class="failure">{{ error }}</p>
    <template v-if="policy && view">
      <div class="grid">
        <label>
          Replace findings with
          <select v-model="policy.defaults.action">
            <option v-for="a in view.actions" :key="a" :value="a">{{ a }}</option>
          </select>
        </label>
        <label>
          Minimum confidence
          <input v-model.number="policy.defaults.min_confidence" type="number" min="0" max="1" step="0.05" />
        </label>
        <fieldset>
          <legend>Languages</legend>
          <label v-for="l in view.locales" :key="l">
            <input v-model="policy.locales" type="checkbox" :value="l" /> {{ l }}
          </label>
        </fieldset>
      </div>

      <details>
        <summary>Per type</summary>
        <table class="types">
          <thead>
            <tr><th>Type</th><th>Replace with</th><th>Min. confidence</th></tr>
          </thead>
          <tbody>
            <tr v-for="e in entityRows" :key="e">
              <td class="mono">{{ e }}</td>
              <td>
                <select v-model="entityRule(e).action">
                  <option :value="undefined">as above</option>
                  <option v-for="a in view.actions" :key="a" :value="a">{{ a }}</option>
                </select>
              </td>
              <td><input v-model.number="entityRule(e).min_confidence" type="number" min="0" max="1" step="0.05" placeholder="as above" /></td>
            </tr>
          </tbody>
        </table>
      </details>

      <details open>
        <summary>Never personal data here</summary>
        <div class="grid two">
          <label>
            Exact values, one per line (your firm's name, a court)
            <textarea v-model="allowValues" rows="4" spellcheck="false" />
          </label>
          <label>
            Patterns a whole finding matches, one per line
            <textarea v-model="allowPatterns" class="mono" rows="4" spellcheck="false" />
          </label>
        </div>
      </details>

      <details open>
        <summary>Your own patterns ({{ policy.patterns.length }})</summary>
        <div v-for="(p, i) in policy.patterns" :key="i" class="pattern">
          <input v-model="p.name" class="mono" placeholder="name" title="lower case letters, digits and -" />
          <input v-model="p.entity" class="mono" list="veilq-entities" placeholder="TYPE" />
          <input v-model="p.regex" class="mono regex" placeholder="regex, e.g. KD-\d{6}" />
          <input v-model.number="p.confidence" type="number" min="0" max="1" step="0.05" placeholder="0.9" />
          <button class="small" title="Remove" @click="policy.patterns.splice(i, 1)">×</button>
        </div>
        <datalist id="veilq-entities">
          <option v-for="e in view.entities" :key="e" :value="e" />
        </datalist>
        <button class="small" @click="addPattern">Add a pattern</button>
      </details>

      <label>
        Try it on a sample (patterns only; no models)
        <textarea v-model="sample" rows="2" spellcheck="false" />
      </label>
      <div class="row">
        <button @click="tryIt">Try</button>
        <button class="primary" :disabled="busy" @click="save">Save</button>
        <span v-if="saved" class="ok">{{ saved }}</span>
      </div>
      <template v-if="tried">
        <p v-if="tried.output !== undefined" class="mono out">{{ tried.output }}</p>
        <p v-if="tried.findings" class="hint">Found: {{ tried.findings.join(', ') || 'nothing' }}</p>
        <details>
          <summary>As veilq.toml</summary>
          <pre class="mono toml">{{ tried.toml }}</pre>
        </details>
      </template>
    </template>
  </section>
</template>

<style scoped>
.card {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-4);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--surface);
  font-size: 13px;
}
.card h3 {
  font-size: 14px;
}
.hint {
  color: var(--text-muted);
  font-size: 12.5px;
}
.grid {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-3);
  align-items: flex-start;
}
.grid.two > label {
  flex: 1;
  min-width: 240px;
}
label {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
fieldset {
  display: flex;
  gap: var(--space-3);
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  padding: 4px var(--space-2);
}
fieldset label {
  flex-direction: row;
  align-items: center;
}
input,
select,
textarea {
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: var(--surface-2);
  padding: 3px 6px;
  font-size: 13px;
}
textarea {
  resize: vertical;
}
summary {
  cursor: pointer;
  color: var(--text-2);
  margin: var(--space-1) 0;
}
.types {
  border-collapse: collapse;
  width: 100%;
}
.types th {
  text-align: left;
  font-weight: 500;
  color: var(--text-muted);
  font-size: 12px;
}
.types td {
  padding: 2px 6px 2px 0;
}
.pattern {
  display: flex;
  gap: var(--space-1);
  margin-bottom: var(--space-1);
}
.pattern input {
  width: 9em;
}
.pattern .regex {
  flex: 1;
}
.row {
  display: flex;
  gap: var(--space-2);
  align-items: center;
}
.ok {
  color: var(--ok-text);
}
.out {
  padding: var(--space-2);
  border-radius: var(--radius-sm);
  background: var(--surface-2);
  white-space: pre-wrap;
}
.toml {
  padding: var(--space-2);
  border-radius: var(--radius-sm);
  background: var(--surface-2);
  overflow: auto;
}
.failure {
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-sm);
  background: var(--danger-soft);
  color: var(--danger-text);
}
</style>
