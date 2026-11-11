const EXTENSIONS: Record<string, string> = {
  'image/png': 'png',
  'image/jpeg': 'jpg',
  'image/webp': 'webp',
  'image/gif': 'gif',
};

/**
 * The name a pasted file is uploaded under. A file copied in the file
 * manager keeps its own; a screenshot comes as `image.png` or without a
 * name, and its type gives the extension veilq reads the format from.
 */
export function pastedName(name: string, type: string): string {
  if (/\.[a-z0-9]+$/i.test(name) && name !== 'image.png') return name;
  return `clipboard.${EXTENSIONS[type] ?? 'bin'}`;
}
