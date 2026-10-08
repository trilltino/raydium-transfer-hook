import { defineConfig, devices } from '@playwright/test';

/**
 * Browser end-to-end tests against a local validator. `e2e/global-setup.ts` starts the validator if none
 * is running, sets up a Fair Launch pool and a funded throwaway wallet, and the page is served from a
 * build with `VITE_E2E=1`, the only build that contains the test wallet.
 */
export default defineConfig({
  testDir: './e2e',
  globalSetup: './e2e/global-setup.ts',
  globalTeardown: './e2e/global-teardown.ts',
  timeout: 120_000,
  expect: { timeout: 30_000 },
  fullyParallel: false,
  workers: 1,
  reporter: [['list']],
  use: { baseURL: 'http://127.0.0.1:5173', trace: 'retain-on-failure' },
  webServer: {
    // The generated environments file is not tracked, so a clean checkout has to make it first.
    command: 'npm run generate && npx vite --host 127.0.0.1 --port 5173 --strictPort',
    url: 'http://127.0.0.1:5173',
    // E2E_REUSE_SERVER=1 reuses a dev server already running on this port (started with VITE_E2E=1).
    reuseExistingServer: process.env.E2E_REUSE_SERVER === '1',
    timeout: 120_000,
    env: { VITE_E2E: '1' },
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
});
