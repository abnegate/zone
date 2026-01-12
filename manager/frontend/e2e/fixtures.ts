import { test as base, expect } from '@playwright/test';

export const test = base.extend({
  page: async ({ page }, use) => {
    await page.route('**/src/api/**', (route) => route.continue());
    await page.route('**/@fs/**/src/api/**', (route) => route.continue());
    await use(page);
  },
});

export { expect };
