// ESLint 扁平配置（ESLint 10 + typescript-eslint 8）。
// 只保留正确性规则，代码风格由末尾的 eslint-config-prettier 交给 Prettier 处理。
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
      // .worktrees 是独立检出目录，不忽略会导致无法解析唯一的 tsconfigRootDir 并使全仓解析报错。
      '.worktrees/**',
      // skills/ 存放本地运行期笔记，非仓库源码，不参与 lint。
      'skills/**',
      'src-tauri/gen/**',
      'public/**',
    ],
  },

  // 基础 JS 正确性规则。
  js.configs.recommended,
  // typescript-eslint 规则；其内部重写会关闭在 TS 中误报的规则（如 no-undef）。
  ...tseslint.configs.recommended,

  {
    // 允许下划线前缀变量作为占位符（未使用的 catch 参数、解构占位等），作用于所有匹配文件。
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
      // 单测 mock 允许使用 any 类型转换（如 invoke 桩函数）。
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

  // 构建与测试脚本（ESM）注入 Node 全局变量。
  {
    files: ['scripts/**/*.mjs', '*.js'],
    languageOptions: {
      sourceType: 'module',
      globals: { ...globals.node },
    },
    rules: {
      // 纯 JS 工具脚本允许使用 require()。
      '@typescript-eslint/no-require-imports': 'off',
    },
  },

  // CommonJS 脚本（*.cjs）设置 sourceType 为 commonjs，由 ESLint 自动推导 module/require 全局变量。
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

  // 必须置于末尾：关闭已由 Prettier 接管的格式与样式规则。
  prettier,
];
