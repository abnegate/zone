import { expect, test } from '@playwright/test';
import { readFileSync } from 'node:fs';

const styles = [
  '../../../packages/ui/src/styles/variables.css',
  '../../../packages/ui/src/styles/globals.css',
  '../src/styles/base.css',
  '../src/styles/modals.css',
]
  .map((path) => readFileSync(new URL(path, import.meta.url), 'utf8'))
  .join('\n');

for (const theme of ['light', 'dark']) {
  test(`dialog content remains readable in ${theme} mode`, async ({ page }) => {
    // Reproduce a portal outside the app's foreground-color container.
    await page.setContent(`
      <html data-theme="${theme}"><head><style>${styles}</style></head>
      <body style="color: #1a1612">
        <div class="ui-dialog-content" role="dialog" aria-labelledby="title">
          <div class="ui-dialog-header"><h2 class="ui-dialog-title" id="title">Delete Chat</h2></div>
          <p>Are you sure you want to delete this chat? This action cannot be undone.</p>
          <div class="modal-actions">
            <button class="ui-btn ui-btn-md ui-btn-secondary">Cancel</button>
            <button class="ui-btn ui-btn-md ui-btn-destructive">Delete</button>
          </div>
          <button class="ui-dialog-close" aria-label="Close">
            <svg class="ui-dialog-close-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        </div>
      </body></html>
    `);

    const foreground = theme === 'dark' ? 'rgb(243, 238, 230)' : 'rgb(26, 22, 18)';
    await expect(page.locator('.ui-dialog-content > p')).toHaveCSS('color', foreground);
    await expect(page.getByRole('button', { name: 'Close' })).toHaveCSS('color', foreground);
    await expect(page.getByRole('heading', { name: 'Delete Chat' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Cancel' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Delete', exact: true })).toBeVisible();
    await expect(page.locator('.modal-actions')).toHaveCSS('border-top-style', 'solid');
    await page.screenshot({ path: test.info().outputPath(`dialog-${theme}.png`) });
  });
}
