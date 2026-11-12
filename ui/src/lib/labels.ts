import type { Location } from '@/api';
import { catalogs, locale, t, type MessageKey } from '@/lib/i18n';

// Names for what veilq reports in its own words: entity types, actions and
// model components. One without a name of its own is shown as it is.

function known(key: string): key is MessageKey {
  return key in catalogs[locale.value];
}

/** `PERSON` → "Name"; a type of a user's own pattern stays as written. */
export function entityLabel(entity: string): string {
  const key = `entity.${entity}`;
  return known(key) ? t(key) : entity;
}

/** `mask` → "Placeholder". */
export function actionLabel(action: string): string {
  const key = `action.${action}`;
  return known(key) ? t(key) : action;
}

/** `ocr` → "Text in pictures". */
export function modelLabel(name: string): string {
  const key = `model.${name}`;
  return known(key) ? t(key) : name;
}

/** What a model component is for; veilq's own English text where there is no translation. */
export function modelAbout(name: string, about: string): string {
  const key = `model.${name}.about`;
  return known(key) ? t(key) : about;
}

/** `legal` → "Law firm, authority". */
export function purposeLabel(name: string): string {
  const key = `purpose.${name}`;
  return known(key) ? t(key) : name;
}

/** What a purpose is for; veilq's own English text where there is no translation. */
export function purposeAbout(name: string, about: string): string {
  const key = `purpose.${name}.about`;
  return known(key) ? t(key) : about;
}

/** Where a finding is, in words. */
export function describeLocation(location: Location): string {
  const part = 'part' in location && location.part ? `${location.part}, ` : '';
  switch (location.kind) {
    case 'span':
      return part + t('where.span', { line: location.line, column: location.column });
    case 'cell':
      return (location.sheet ? `${location.sheet}, ` : '') + t('where.cell', { row: location.row, column: location.column });
    case 'region':
      return part + (location.frame ? t('where.page', { page: location.frame }) : t('where.picture'));
  }
}

/**
 * The model components a message of veilq asks for (`veilq models fetch
 * ocr`), so the UI can offer to download them where the message is shown.
 */
export function missingModels(message: string): string[] {
  const names = [...message.matchAll(/veilq models fetch (\w+)/g)].map((m) => m[1]);
  return [...new Set(names)];
}
