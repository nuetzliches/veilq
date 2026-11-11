import {
  ApiError,
  type Api,
  type Info,
  type Listing,
  type Models,
  type Picture,
  type Redaction,
  type SetupStatus,
  type Source,
  type UpdateCheck,
} from './types';

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
    if (response.status === 401) {
      throw new ApiError(401, 'This page has no valid token: open veilq ui with the link it printed (under devstack, the dashboard\'s link).');
    }
    if (!response.ok) {
      throw new ApiError(response.status, body?.error ?? `${response.status} ${response.statusText}`);
    }
    return body as T;
  }
  return {
    mock: false,
    info: () => call<Info>('/info'),
    files: (dir) => call<Listing>(`/files?dir=${encodeURIComponent(dir)}`),
    picture: (path) => call<Picture>(`/picture?path=${encodeURIComponent(path)}`),
    redact: (source: Source, action?: string, unverified?: boolean) =>
      call<Redaction>('/redact', {
        method: 'POST',
        body: JSON.stringify({ source, action, unverified: unverified || undefined }),
      }),
    setup: () => call<SetupStatus>('/setup'),
    install: (request) => call<SetupStatus>('/setup', { method: 'POST', body: JSON.stringify(request) }),
    uninstall: async (models) => {
      await call<null>('/setup/uninstall', { method: 'POST', body: JSON.stringify({ models }) });
    },
    models: () => call<Models>('/models'),
    fetchModel: async (name) => {
      await call<null>(`/models/${encodeURIComponent(name)}/fetch`, { method: 'POST' });
    },
    checkUpdate: () => call<UpdateCheck>('/update'),
    installUpdate: () => call<UpdateCheck>('/update', { method: 'POST' }),
    quit: async () => {
      await call<null>('/quit', { method: 'POST' });
    },
    ping: async () => {
      await call<null>('/ping', { method: 'POST' });
    },
  };
}
