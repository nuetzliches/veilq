import { computed, ref } from 'vue';
import { de } from '@/lib/messages/de';
import { en, type MessageKey } from '@/lib/messages/en';

// Two languages: German where the system speaks German, English everywhere
// else. A choice in Settings is kept in this browser and comes first.

export type Locale = 'de' | 'en';
export type Choice = Locale | 'system';

const STORAGE_KEY = 'veilq-locale';
const catalogs: Record<Locale, Record<MessageKey, string>> = { de, en };

/** The language for the system's preferred languages: the first one decides. */
export function systemLocale(languages: readonly string[]): Locale {
  return languages[0]?.toLowerCase().startsWith('de') ? 'de' : 'en';
}

function stored(): Choice {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    return v === 'de' || v === 'en' ? v : 'system';
  } catch {
    return 'system';
  }
}

function languages(): readonly string[] {
  if (typeof navigator === 'undefined') return [];
  return navigator.languages?.length ? navigator.languages : [navigator.language];
}

/** What the user chose in Settings, or `system`. */
export const choice = ref<Choice>(stored());
/** The language in use. */
export const locale = computed<Locale>(() => (choice.value === 'system' ? systemLocale(languages()) : choice.value));

export function setChoice(c: Choice) {
  choice.value = c;
  try {
    if (c === 'system') localStorage.removeItem(STORAGE_KEY);
    else localStorage.setItem(STORAGE_KEY, c);
  } catch {
    // A browser without storage keeps the choice for this page only.
  }
  document.documentElement.lang = locale.value;
}

/** Fills `{name}` in a message from `params`. */
export function format(message: string, params: Record<string, string | number> = {}): string {
  return message.replace(/\{(\w+)\}/g, (all, name: string) => (name in params ? String(params[name]) : all));
}

/** The message `key` in the language in use. */
export function t(key: MessageKey, params?: Record<string, string | number>): string {
  return format(catalogs[locale.value][key], params);
}

/** `one` for 1, else `other`, both filled with `{n}`. */
export function plural(n: number, one: MessageKey, other: MessageKey): string {
  return t(n === 1 ? one : other, { n });
}

export { catalogs };
export type { MessageKey };
