/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Base URL for the Lodestone Rust API. Defaults to `/api/v1`. */
  readonly VITE_API_BASE?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
