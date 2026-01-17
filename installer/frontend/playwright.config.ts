import { defineConfig, devices } from '@playwright/test';

// Allow configurable port for running alongside other services
const port = process.env.PORT || '3000';
const baseURL = `http://localhost:${port}`;

export default defineConfig({
  testDir: './e2e',
  testMatch: '**/*.e2e.ts',
  timeout: 60000, // 60s per test
  expect: { timeout: 10000 }, // 10s for assertions
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: process.env.CI ? [['list'], ['html']] : [['html']],
  use: {
    baseURL,
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    navigationTimeout: 30000, // 30s for navigation
    actionTimeout: 15000, // 15s for actions
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
    {
      name: 'firefox',
      use: { ...devices['Desktop Firefox'] },
    },
    {
      name: 'webkit',
      use: { ...devices['Desktop Safari'] },
    },
  ],
  webServer: {
    command: `PORT=${port} bun start`,
    url: baseURL,
    reuseExistingServer: true,
    timeout: 60000, // 60s to start server
  },
});
