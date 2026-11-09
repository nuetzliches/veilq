import { fileURLToPath, URL } from 'node:url';
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

export default defineConfig(({ mode }) => {
  // `--mode mock` or VITE_MOCK=1 swaps the API of `veilq ui` for the in-browser
  // fake with synthetic samples (src/api/mock.ts).
  const mock = mode === 'mock' || process.env.VITE_MOCK === '1';
  return {
    base: '/',
    define: {
      'import.meta.env.VITE_MOCK': JSON.stringify(mock ? '1' : ''),
    },
    resolve: {
      alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
    },
    build: {
      // The polyfill is an inline script; `veilq ui` sends a CSP without
      // 'unsafe-inline'.
      modulePreload: { polyfill: false },
    },
    plugins: [vue()],
  };
});
