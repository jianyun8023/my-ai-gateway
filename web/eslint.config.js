import js from '@eslint/js'
import globals from 'globals'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'
import tseslint from 'typescript-eslint'
import { defineConfig, globalIgnores } from 'eslint/config'
import architecture from './eslint.architecture.js'

export default defineConfig([
  globalIgnores(['dist']),
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ['**/*.{ts,tsx,js,jsx}'],
    extends: [
      reactHooks.configs.flat.recommended,
      reactRefresh.configs.vite,
    ],
    languageOptions: {
      ecmaVersion: 2020,
      globals: {
        ...globals.browser,
        ...globals.node,
      },
      parserOptions: {
        ecmaVersion: 'latest',
        ecmaFeatures: { jsx: true },
        sourceType: 'module',
      },
    },
    rules: {
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_', ignoreRestSiblings: true }],
      'no-unused-vars': 'off',
      'react-refresh/only-export-components': ['error', { allowConstantExport: true }],
    },
  },
  {
    files: ['src/**/*.{ts,tsx}'],
    ignores: ['src/**/*.test.{ts,tsx}', 'src/**/test/**'],
    plugins: { architecture },
    rules: { 'architecture/module-boundaries': 'error' },
  },
  {
    files: ['src/**/*.{ts,tsx}'],
    ignores: ['src/admin-api/client.ts', 'src/**/*.test.{ts,tsx}', 'src/**/test/**'],
    rules: {
      'no-restricted-globals': ['error',
        { name: 'fetch', message: 'Use the shared AdminClient transport.' },
        { name: 'XMLHttpRequest', message: 'Use the shared AdminClient transport.' },
      ],
      'no-restricted-properties': ['error',
        { property: 'fetch', message: 'Use the shared AdminClient transport.' },
      ],
    },
  },
])
