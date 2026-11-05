import { defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";
// @ts-expect-error node:fs is a Node builtin; this file is not covered by @types/node.
import { readFileSync } from "node:fs";

const pkg = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8")) as {
  version: string;
};

export default defineConfig({
  plugins: [vue()],
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },
  test: {
    environment: "jsdom",
    globals: true,
    // jsdom's default origin is about:blank, which has no localStorage — the update store
    // uses it, so give the test DOM a real origin.
    environmentOptions: { jsdom: { url: "http://localhost/" } },
    // Git worktrees created inside the repo (e.g. .claude/worktrees/...) carry
    // their own copy of the test suite; running those duplicates every test.
    exclude: ["**/node_modules/**", "**/dist/**", ".claude/**", ".superpowers/**"],
  },
});
