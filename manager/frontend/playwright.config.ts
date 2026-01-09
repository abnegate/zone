import { defineConfig, devices } from '@playwright/test';

// Allow configurable port for running alongside other services
const port = process.env.PORT || '3000';
const baseURL = `http://localhost:${port}`;

// Allow running specific browser via environment variable (for CI matrix)
const browserProject = process.env.BROWSER;
const collectCoverage = process.env.COLLECT_COVERAGE === 'true';

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
  fullyParallel: !collectCoverage, // Run sequentially when collecting coverage
  forbidOnly: !!process.env.CI,
  retries: process.env.CI && !collectCoverage ? 3 : 0,
  workers: collectCoverage ? 1 : process.env.CI ? 1 : undefined,
  reporter: collectCoverage ? coverageReporter : defaultReporters,
  use: {
    baseURL,
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
  },
  projects: collectCoverage
    ? [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }]
    : projects,
  webServer: {
    command: `PORT=${port} npm start`,
    url: baseURL,
    reuseExistingServer: !process.env.CI,
    timeout: 120000,
  },
});
