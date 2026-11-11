/**
 * A picture as PNG for the clipboard: browsers take `image/png` only, so a
 * JPEG or WebP is drawn once onto a canvas. A GIF is not offered (its
 * frames would be lost); the caller checks [`copyablePicture`].
 */
export function copyablePicture(mediaType: string): boolean {
  return ['image/png', 'image/jpeg', 'image/webp'].includes(mediaType);
}

export async function pngBlob(base64: string, mediaType: string): Promise<Blob> {
  const bytes = Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
  const blob = new Blob([bytes], { type: mediaType });
  if (mediaType === 'image/png') return blob;
  const bitmap = await createImageBitmap(blob);
  const canvas = document.createElement('canvas');
  canvas.width = bitmap.width;
  canvas.height = bitmap.height;
  canvas.getContext('2d')!.drawImage(bitmap, 0, 0);
  return new Promise((resolve, reject) =>
    canvas.toBlob((b) => (b ? resolve(b) : reject(new Error('the picture could not be converted'))), 'image/png'),
  );
}

/**
 * Whether a Ctrl+C belongs to the page: nothing selected and no field in
 * focus. Then the output is copied; else the browser copies the selection.
 */
export function copyIsOurs(target: EventTarget | null, selection: string): boolean {
  const el = target as HTMLElement | null;
  return !selection && !el?.closest?.('textarea, input, [contenteditable="true"]');
}
