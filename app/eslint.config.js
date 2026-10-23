// Lint for the UI: TypeScript's recommended rules, React's rules of hooks
// (stale data and missing effects), and jsx-a11y so accessibility mistakes
// are caught as they're written, not later in an end-to-end test.
import js from "@eslint/js";
import jsxA11y from "eslint-plugin-jsx-a11y";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist", "src-tauri", "playwright-report", "test-results", "e2e/.data"] },
  {
    files: ["**/*.{ts,tsx}"],
    extends: [js.configs.recommended, ...tseslint.configs.recommended, jsxA11y.flatConfigs.recommended],
    languageOptions: { globals: { ...globals.browser, ...globals.node } },
    plugins: { "react-hooks": reactHooks },
    rules: {
      ...reactHooks.configs.recommended.rules,
      // Every autoFocus here puts the cursor in a dialog's first field when it
      // opens, which is what the WAI-ARIA dialog pattern asks for. The rule is
      // aimed at pages that steal focus on load, which Tallybook doesn't do.
      "jsx-a11y/no-autofocus": "off",
      // A scrollable region must be focusable so the keyboard can scroll it
      // (WCAG; axe's scrollable-region-focusable): see .table-scroll.
      "jsx-a11y/no-noninteractive-tabindex": ["error", { roles: ["tabpanel", "region"] }],
    },
  },
);
