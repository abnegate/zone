import { test, expect } from '@playwright/test';
import { selectOption } from './helpers';

test.describe('Advanced Settings', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    // Navigate to Advanced step (step 6 - last step)
    await page.click('[data-step="6"]');
    await expect(page.getByRole('heading', { name: 'Advanced Settings' })).toBeVisible();
  });

  test.describe('Monitoring Section', () => {
    test('displays monitoring toggle', async ({ page }) => {
      await expect(page.getByLabel('Enable Prometheus + Grafana monitoring')).toBeVisible();
    });

    test('monitoring fields hidden by default', async ({ page }) => {
      // Monitoring should be disabled by default, so Grafana fields should be hidden
      const monitoringCheckbox = page.getByLabel('Enable Prometheus + Grafana monitoring');
      const isChecked = await monitoringCheckbox.isChecked();

      if (!isChecked) {
        await expect(page.getByLabel('Grafana Admin Username')).not.toBeVisible();
        await expect(page.getByLabel('Grafana Admin Password')).not.toBeVisible();
      }
    });

    test('enabling monitoring shows Grafana fields', async ({ page }) => {
      const monitoringCheckbox = page.getByLabel('Enable Prometheus + Grafana monitoring');

      if (!(await monitoringCheckbox.isChecked())) {
        await monitoringCheckbox.check();
      }

      await expect(page.getByLabel('Grafana Admin Username')).toBeVisible();
      await expect(page.getByLabel('Grafana Admin Password')).toBeVisible();
      await expect(page.getByLabel('Metrics Retention')).toBeVisible();
    });

    test('can configure Grafana credentials', async ({ page }) => {
      const monitoringCheckbox = page.getByLabel('Enable Prometheus + Grafana monitoring');
      await monitoringCheckbox.check();

      const usernameInput = page.getByLabel('Grafana Admin Username');
      const passwordInput = page.getByLabel('Grafana Admin Password');

      await usernameInput.clear();
      await usernameInput.fill('myadmin');

      await passwordInput.clear();
      await passwordInput.fill('mypassword123');

      await expect(usernameInput).toHaveValue('myadmin');
      await expect(passwordInput).toHaveValue('mypassword123');
    });

    test('can select metrics retention period', async ({ page }) => {
      await page.getByLabel('Enable Prometheus + Grafana monitoring').check();

      const retentionSelect = page.getByLabel('Metrics Retention');
      const options = ['7 days', '15 days', '30 days', '90 days'];

      for (const option of options) {
        await selectOption(page, 'Metrics Retention', option);
        await expect(retentionSelect).toHaveText(option);
      }
    });

    test('shows docker command info for monitoring', async ({ page }) => {
      await page.getByLabel('Enable Prometheus + Grafana monitoring').check();
      await expect(page.getByText('docker compose --profile monitoring up')).toBeVisible();
    });

    test('Grafana password has generate button', async ({ page }) => {
      await page.getByLabel('Enable Prometheus + Grafana monitoring').check();

      // Look for generate button near the password field
      const generateButton = page.locator('button:has-text("Generate")').first();
      await expect(generateButton).toBeVisible();
    });
  });

  test.describe('Email Alerts Section', () => {
    test.beforeEach(async ({ page }) => {
      // Enable monitoring first (required for alerts)
      await page.getByLabel('Enable Prometheus + Grafana monitoring').check();
    });

    test('displays alerting toggle', async ({ page }) => {
      await expect(page.getByLabel('Enable email alerts for critical events')).toBeVisible();
    });

    test('alerting fields hidden by default', async ({ page }) => {
      const alertingCheckbox = page.getByLabel('Enable email alerts for critical events');
      const isChecked = await alertingCheckbox.isChecked();

      if (!isChecked) {
        await expect(page.getByLabel('Alert Recipients')).not.toBeVisible();
        await expect(page.getByLabel('SMTP Host')).not.toBeVisible();
      }
    });

    test('enabling alerts shows SMTP configuration', async ({ page }) => {
      await page.getByLabel('Enable email alerts for critical events').check();

      await expect(page.getByLabel('Alert Recipients')).toBeVisible();
      await expect(page.getByLabel('SMTP Host')).toBeVisible();
      await expect(page.getByLabel('SMTP Port')).toBeVisible();
      await expect(page.getByLabel('SMTP Username')).toBeVisible();
      await expect(page.getByLabel('SMTP Password')).toBeVisible();
      await expect(page.getByLabel('From Address')).toBeVisible();
      await expect(page.getByLabel('From Name')).toBeVisible();
    });

    test('can configure SMTP settings', async ({ page }) => {
      await page.getByLabel('Enable email alerts for critical events').check();

      await page.getByLabel('Alert Recipients').fill('admin@example.com');
      await page.getByLabel('SMTP Host').fill('smtp.gmail.com');
      await selectOption(page, 'SMTP Port', '587 (Submission)');
      await page.getByLabel('SMTP Username').fill('user@gmail.com');
      await page.getByLabel('SMTP Password').fill('app-password');
      await page.getByLabel('From Address').fill('alerts@example.com');
      await page.getByLabel('From Name').fill('Zone Monitoring');

      await expect(page.getByLabel('Alert Recipients')).toHaveValue('admin@example.com');
      await expect(page.getByLabel('SMTP Host')).toHaveValue('smtp.gmail.com');
      await expect(page.getByLabel('SMTP Port')).toHaveText('587 (Submission)');
    });

    test('shows alert types info', async ({ page }) => {
      await page.getByLabel('Enable email alerts for critical events').check();
      await expect(page.getByText('service outages')).toBeVisible();
    });
  });

  test.describe('Performance Section', () => {
    test('displays worker count input', async ({ page }) => {
      const input = page.getByLabel('Worker Count');
      await expect(input).toBeVisible();
      await expect(input).toHaveAttribute('type', 'number');
    });

    test('can change worker count', async ({ page }) => {
      const input = page.getByLabel('Worker Count');
      await input.clear();
      await input.fill('4');
      await expect(input).toHaveValue('4');
    });

    test('worker count has constraints', async ({ page }) => {
      const input = page.getByLabel('Worker Count');
      await expect(input).toHaveAttribute('min', '1');
      await expect(input).toHaveAttribute('max', '16');
    });

    test('displays request timeout input', async ({ page }) => {
      const input = page.getByLabel('Request Timeout (seconds)');
      await expect(input).toBeVisible();
    });

    test('can change request timeout', async ({ page }) => {
      const input = page.getByLabel('Request Timeout (seconds)');
      await input.clear();
      await input.fill('300');
      await expect(input).toHaveValue('300');
    });

    test('request timeout has constraints', async ({ page }) => {
      const input = page.getByLabel('Request Timeout (seconds)');
      await expect(input).toHaveAttribute('min', '60');
      await expect(input).toHaveAttribute('max', '1800');
    });

    test('displays timezone selector', async ({ page }) => {
      await expect(page.getByLabel('Timezone')).toBeVisible();
    });

    test('can select different timezones', async ({ page }) => {
      const tzSelect = page.getByLabel('Timezone');

      await selectOption(page, 'Timezone', 'America/New_York');
      await expect(tzSelect).toHaveText('America/New_York');

      await selectOption(page, 'Timezone', 'Asia/Tokyo');
      await expect(tzSelect).toHaveText('Asia/Tokyo');
    });
  });

  test.describe('ACME Configuration', () => {
    test('displays ACME email input', async ({ page }) => {
      await expect(page.getByLabel("ACME Email (for Let's Encrypt)")).toBeVisible();
    });

    test('can fill ACME email', async ({ page }) => {
      const input = page.getByLabel("ACME Email (for Let's Encrypt)");
      await input.clear();
      await input.fill('admin@example.com');
      await expect(input).toHaveValue('admin@example.com');
    });
  });

  test('shows completion info box', async ({ page }) => {
    await expect(page.getByText('Configuration complete')).toBeVisible();
  });

  test('shows Install button on this step', async ({ page }) => {
    await expect(page.locator('button:has-text("Install")')).toBeVisible();
  });
});
