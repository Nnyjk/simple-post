// Flat ESLint config (ESLint 10 + typescript-eslint 8).
// Keep rule choices conservative: correctness rules first, style is
// delegated to Prettier via eslint-config-prettier (last in the list).
import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import reactHooks from 'eslint-plugin-react-hooks';
import globals from 'globals';
import prettier from 'eslint-config-prettier';

export default [
  {
    ignores: [
      'dist/**',
      'target/**',
      'node_modules/**',
      '.scratch/**',
      // Agent worktrees are full second checkouts of this repo. Without
      // this ignore the flat config cannot resolve a single tsconfigRootDir
      // and every file in the real tree fails to parse.
      '.worktrees/**',
      'src-tauri/gen/**',
      'docs/**',
      'public/**',
    ],
  },

  // Base JS correctness rules …
  js.configs.recommended,
  // … then typescript-eslint, whose `eslintRecommended` overrides turn
  // off the base rules that misfire on TypeScript (incl. no-undef).
  ...tseslint.configs.recommended,

  {
    // `_`-prefixed names are intentional placeholders (unused catch
    // params, destructuring stubs). Applies to every linted file,
    // including the .mjs/.cjs tooling scripts.
    rules: {
      '@typescript-eslint/no-unused-vars': [
        'error',
        {
          argsIgnorePattern: '^_',
          varsIgnorePattern: '^_',
          caughtErrorsIgnorePattern: '^_',
          destructuredArrayIgnorePattern: '^_',
        },
      ],
    },
  },

  {
    files: ['**/*.test.ts', '**/__tests__/**'],
    rules: {
      // Test mocks are deliberately loose (`any` cast on invoke fns).
      '@typescript-eslint/no-explicit-any': 'off',
    },
  },

  {
    files: ['src/**/*.{ts,tsx}'],
    plugins: { 'react-hooks': reactHooks },
    rules: {
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'warn',
    },
  },

  // Node globals for build/test scripts (ESM).
  {
    files: ['scripts/**/*.mjs', '*.js'],
    languageOptions: {
      sourceType: 'module',
      globals: { ...globals.node },
    },
    rules: {
      // These are plain JS tooling files — require() is idiomatic.
      '@typescript-eslint/no-require-imports': 'off',
    },
  },

  // CommonJS files (`*.cjs`) — ESLint derives the require/module
  // globals from sourceType: 'commonjs' automatically.
  {
    files: ['**/*.cjs'],
    languageOptions: {
      sourceType: 'commonjs',
      globals: { ...globals.node },
    },
    rules: {
      '@typescript-eslint/no-require-imports': 'off',
    },
  },

  // Must stay last: disables stylistic rules Prettier already owns.
  prettier,
];
