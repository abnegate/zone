import { defineConfig, devices } from '@playwright/test';
import os from 'node:os';

// Allow configurable port for running alongside other services
const isCI = !!process.env.CI;
const defaultPort = isCI ? '4174' : '3001';
const port = process.env.PLAYWRIGHT_PORT || process.env.PORT || defaultPort;
const baseURL = `http://localhost:${port}`;

// Allow running specific browser via environment variable (for CI matrix)
const browserProject = process.env.BROWSER;
const collectCoverage = process.env.COLLECT_COVERAGE === 'true';
const localWorkers = Math.max(1, os.cpus().length);
const workerOverride = process.env.PLAYWRIGHT_WORKERS
  ? Number.parseInt(process.env.PLAYWRIGHT_WORKERS, 10)
  : undefined;

const allProjects = [
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
];

// If BROWSER env var is set, only run that browser; otherwise run all
const projects = browserProject
  ? allProjects.filter((p) => p.name === browserProject)
  : allProjects;

// Configure reporters based on coverage mode
const defaultReporters: Parameters<typeof defineConfig>[0]['reporter'] = process.env.CI
  ? [['list'], ['html', { open: 'never' }]]
  : [['html']];

const coverageReporter: Parameters<typeof defineConfig>[0]['reporter'] = [
  ['list'],
  [
    'monocart-reporter',
    {
      name: 'Playwright E2E Coverage Report',
      outputFile: './coverage-e2e/report.html',
      coverage: {
        entryFilter: (entry: { url: string }) => {
          // Only collect coverage for our app's source files
          return (
            entry.url.includes(`localhost:${port}`) &&
            entry.url.includes('/static/js/') &&
            !entry.url.includes('node_modules')
          );
        },
        sourceFilter: (sourcePath: string) => {
          // Only include src files, exclude tests and config
          return (
            sourcePath.includes('/src/') &&
            !sourcePath.includes('.test.') &&
            !sourcePath.includes('setupTests') &&
            !sourcePath.includes('__mocks__')
          );
        },
        reports: [
          ['v8'],
          ['lcovonly', { outputFile: 'coverage-e2e/lcov.info' }],
          ['json', { outputFile: 'coverage-e2e/coverage-final.json' }],
        ],
      },
    },
  ],
];

export default defineConfig({
  testDir: './e2e',
  testMatch: '**/*.e2e.ts',
  timeout: 60000, // 60s per test
  expect: { timeout: 10000 }, // 10s for assertions
  fullyParallel: !collectCoverage, // Run sequentially when collecting coverage
  forbidOnly: !!process.env.CI,
  retries: process.env.CI && !collectCoverage ? 3 : 0,
  workers: workerOverride ?? (collectCoverage ? 1 : localWorkers),
  reporter: collectCoverage ? coverageReporter : defaultReporters,
  use: {
    baseURL,
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    navigationTimeout: 30000, // 30s for navigation
    actionTimeout: 15000, // 15s for actions
  },
  projects: collectCoverage
    ? [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }]
    : projects,
  webServer: {
    command: `bun start -- --port ${port}`,
    url: baseURL,
    reuseExistingServer: true,
    timeout: 120000, // 120s to start server
    env: {
      PORT: port,
    },
  },
});
