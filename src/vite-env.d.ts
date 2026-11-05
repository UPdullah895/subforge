/// <reference types="vite/client" />

/** package.json's version, injected by vite.config.ts / vitest.config.ts (see updateStore). */
declare const __APP_VERSION__: string;

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent;
  export default component;
}
