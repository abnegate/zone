import { defineConfig, devices } from '@playwright/test';

/**
 * The live suite. It needs a running server, console, Ollama and the ComfyUI
 * stand-in, which `scripts/live-verify.sh` brings up; it is deliberately not
 * part of the mocked Playwright matrix, and it starts no web server of its own
 * so it can never quietly reuse a stale one.
 */
const port = process.env.ZONE_LIVE_PORT || '4179';

export default defineConfig({
  testDir: './live',
  testMatch: '**/*.live.ts',
  // Media lanes and clip sampling are the slow ones, and they are the point.
  timeout: 300_000,
  expect: { timeout: 30_000 },
  fullyParallel: false,
  workers: 1,
  retries: 0,
  forbidOnly: !!process.env.CI,
  reporter: [['list'], ['html', { outputFolder: 'playwright-report/live', open: 'never' }]],
  use: {
    baseURL: `http://localhost:${port}`,
    ...devices['Desktop Chrome'],
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'off',
    navigationTimeout: 60_000,
    actionTimeout: 30_000,
  },
  projects: [{ name: 'chromium' }],
});
