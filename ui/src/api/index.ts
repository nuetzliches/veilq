import { httpApi, takeToken } from './client';
import { mockApi } from './mock';
import type { Api } from './types';

export * from './types';

export function createApi(): Api {
  if (import.meta.env.VITE_MOCK === '1') return mockApi();
  return httpApi(takeToken(window.location, window.sessionStorage, window.history));
}
