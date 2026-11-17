// Marks on one document (#34): what the user says veilq got wrong. A mark
// acts on the document it was made on at once and is kept as a case; it
// changes no other file until a rule is saved.

/** The types a miss can be marked as, the most common first. */
export const MARK_TYPES = [
  'PERSON',
  'ADDRESS',
  'DATE_OF_BIRTH',
  'PHONE_NUMBER',
  'EMAIL_ADDRESS',
  'IBAN_CODE',
  'CUSTOMER_NUMBER',
  'ACCOUNT_NUMBER',
  'FILE_NUMBER',
  'COURT_FILE_NUMBER',
  'LICENSE_PLATE',
  'TAX_ID',
  'ORGANIZATION',
  'SECRET',
] as const;

/** The longest selection a mark takes: a value, not a paragraph. */
export const MAX_MARK = 200;

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** The text of `text` between two byte offsets (the API's spans are bytes). */
export function byteSlice(text: string, [start, end]: [number, number]): string {
  return decoder.decode(encoder.encode(text).slice(start, end));
}

/**
 * What a selection in the output can be marked as missed: trimmed, on one
 * line, not too long, and not one of veilq's own placeholders (`<PERSON>`);
 * `null` otherwise.
 */
export function markable(selection: string): string | null {
  const value = selection.trim();
  if (!value || value.length > MAX_MARK || /[\r\n]/.test(value)) return null;
  if (/[<[][A-Z][A-Z0-9_]*(?::[0-9a-f]{8})?[>\]]/.test(value)) return null;
  return value;
}
