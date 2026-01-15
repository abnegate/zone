import { test, expect } from '@playwright/test';

test.describe('Search Configuration', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    // Navigate to Search step (step 5)
    await page.click('[data-step="5"]');
    await expect(page.getByRole('heading', { name: 'Web Search' })).toBeVisible();
  });

  test('displays web search toggle', async ({ page }) => {
    await expect(page.getByLabel('Enable web search in RAG pipeline')).toBeVisible();
  });

  test('can toggle web search on and off', async ({ page }) => {
    const checkbox = page.getByLabel('Enable web search in RAG pipeline');

    // Toggle on
    await checkbox.check();
    await expect(checkbox).toBeChecked();

    // Toggle off
    await checkbox.uncheck();
    await expect(checkbox).not.toBeChecked();
  });

  test('displays results per query input', async ({ page }) => {
    const input = page.getByLabel('Results per Query');
    await expect(input).toBeVisible();
    await expect(input).toHaveAttribute('type', 'number');
  });

  test('can change results per query', async ({ page }) => {
    const input = page.getByLabel('Results per Query');
    await input.clear();
    await input.fill('10');
    await expect(input).toHaveValue('10');
  });

  test('displays concurrent requests input', async ({ page }) => {
    const input = page.getByLabel('Concurrent Requests');
    await expect(input).toBeVisible();
    await expect(input).toHaveAttribute('type', 'number');
  });

  test('can change concurrent requests', async ({ page }) => {
    const input = page.getByLabel('Concurrent Requests');
    await input.clear();
    await input.fill('8');
    await expect(input).toHaveValue('8');
  });

  test('displays search instance name input', async ({ page }) => {
    const input = page.getByLabel('Search Instance Name');
    await expect(input).toBeVisible();
  });

  test('can change search instance name', async ({ page }) => {
    const input = page.getByLabel('Search Instance Name');
    await input.clear();
    await input.fill('my-search-instance');
    await expect(input).toHaveValue('my-search-instance');
  });

  test('shows info about VPN requirement', async ({ page }) => {
    await expect(page.getByText('Web search requires VPN configuration')).toBeVisible();
  });

  test('results per query has min/max constraints', async ({ page }) => {
    const input = page.getByLabel('Results per Query');
    await expect(input).toHaveAttribute('min', '1');
    await expect(input).toHaveAttribute('max', '20');
  });

  test('concurrent requests has min/max constraints', async ({ page }) => {
    const input = page.getByLabel('Concurrent Requests');
    await expect(input).toHaveAttribute('min', '1');
    await expect(input).toHaveAttribute('max', '32');
  });
});
