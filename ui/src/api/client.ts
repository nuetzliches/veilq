import { ApiError, type Api, type Info, type Listing, type Redaction, type Source } from './types';

const TOKEN_KEY = 'veilq-token';

/**
 * The token `veilq ui` printed: from the URL's fragment (a browser never
 * sends it, so no log has it), kept for this tab only, and taken out of the
 * address bar.
 */
export function takeToken(location: Location, storage: Storage, history: History): string {
  const match = /(?:^#|&)token=([0-9a-f]+)/.exec(location.hash);
  if (match) {
    storage.setItem(TOKEN_KEY, match[1]);
    history.replaceState(null, '', location.pathname + location.search);
  }
  return storage.getItem(TOKEN_KEY) ?? '';
}

export function httpApi(token: string): Api {
  async function call<T>(path: string, init: RequestInit = {}): Promise<T> {
    const response = await fetch(`/api${path}`, {
      ...init,
      headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
    });
    const body = (await response.json().catch(() => null)) as { error?: string } | null;
    if (!response.ok) {
      throw new ApiError(response.status, body?.error ?? `${response.status} ${response.statusText}`);
    }
    return body as T;
  }
  return {
    mock: false,
    info: () => call<Info>('/info'),
    files: (dir) => call<Listing>(`/files?dir=${encodeURIComponent(dir)}`),
    redact: (source: Source) => call<Redaction>('/redact', { method: 'POST', body: JSON.stringify({ source }) }),
  };
}
