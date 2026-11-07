// Vitest config used only by Stryker (stryker.config.json). It is the app's own config with
// the test list narrowed to the tests of the mutated files, so each mutant does not run the
// whole suite. See TESTING.md, "Property and mutation testing".
import { defineConfig, mergeConfig } from 'vitest/config';
import viteConfig from './vite.config';

export default defineConfig((env) =>
  mergeConfig(viteConfig(env), {
    test: {
      include: ['src/utils/api/**/*.test.ts', 'src/utils/cache/**/*.test.ts'],
    },
  }),
);
