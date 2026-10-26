import { defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],
  test: {
    environment: "jsdom",
    globals: true,
    // Git worktrees created inside the repo (e.g. .claude/worktrees/...) carry
    // their own copy of the test suite; running those duplicates every test.
    exclude: ["**/node_modules/**", "**/dist/**", ".claude/**", ".superpowers/**"],
  },
});
