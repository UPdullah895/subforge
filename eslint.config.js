import tseslint from "typescript-eslint";
import pluginVue from "eslint-plugin-vue";
import prettier from "eslint-config-prettier";

export default tseslint.config(
  { ignores: ["dist/", "src-tauri/", "server/", "node_modules/", "target-shared/", "crates/*/target/"] },

  ...tseslint.configs.recommended,

  ...pluginVue.configs["flat/recommended"],

  // Wire up TypeScript parser for <script lang="ts"> inside .vue files
  {
    files: ["**/*.vue"],
    languageOptions: {
      parserOptions: {
        parser: tseslint.parser,
      },
    },
    rules: {
      "vue/multi-word-component-names": "off",
    },
  },

  prettier
);
