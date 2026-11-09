/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** "1" when the in-browser fake API is used (`npm run dev`, build:mock, VITE_MOCK=1). */
  readonly VITE_MOCK: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
