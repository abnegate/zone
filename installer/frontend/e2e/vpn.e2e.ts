import { test, expect } from '@playwright/test';
import { selectOption } from './helpers';

test.describe('VPN Configuration', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    // Navigate to VPN step (step 5)
    await page.click('[data-step="5"]');
    await expect(page.getByRole('heading', { name: 'VPN Configuration' })).toBeVisible();
  });

  test('displays VPN provider selection', async ({ page }) => {
    await expect(page.getByLabel('VPN Provider')).toBeVisible();
  });

  test('displays protocol selection', async ({ page }) => {
    await expect(page.getByLabel('Protocol')).toBeVisible();
  });

  test('shows OpenVPN fields by default', async ({ page }) => {
    // Default protocol should be OpenVPN, showing username/password
    await expect(page.getByLabel('Username')).toBeVisible();
    await expect(page.getByLabel('Password')).toBeVisible();
  });

  test('switches to WireGuard and shows different fields', async ({ page }) => {
    await selectOption(page, 'Protocol', 'WireGuard');

    // Should now show WireGuard-specific fields
    await expect(page.getByLabel('Private Key')).toBeVisible();
    await expect(page.getByLabel('Addresses')).toBeVisible();

    // OpenVPN fields should be hidden
    await expect(page.getByLabel('Username')).not.toBeVisible();
    await expect(page.getByLabel('Password')).not.toBeVisible();
  });

  test('can select different VPN providers', async ({ page }) => {
    const providerSelect = page.getByLabel('VPN Provider');

    const providers = ['Surfshark', 'NordVPN', 'ExpressVPN', 'ProtonVPN', 'Mullvad'];

    for (const provider of providers) {
      await selectOption(page, 'VPN Provider', provider);
      await expect(providerSelect).toHaveText(provider);
    }
  });

  test('displays server location fields', async ({ page }) => {
    await expect(page.getByText('Server Location (Optional)')).toBeVisible();
    await expect(page.getByLabel('Country')).toBeVisible();
    await expect(page.getByLabel('City')).toBeVisible();
    await expect(page.getByLabel('Region')).toBeVisible();
  });

  test('can fill OpenVPN credentials', async ({ page }) => {
    const usernameInput = page.getByLabel('Username');
    const passwordInput = page.getByLabel('Password');

    await usernameInput.fill('testuser');
    await passwordInput.fill('testpass123');

    await expect(usernameInput).toHaveValue('testuser');
    await expect(passwordInput).toHaveValue('testpass123');
  });

  test('can fill WireGuard configuration', async ({ page }) => {
    await selectOption(page, 'Protocol', 'WireGuard');

    const privateKeyInput = page.getByLabel('Private Key');
    const addressesInput = page.getByLabel('Addresses');

    await privateKeyInput.fill('test-private-key-abc123');
    await addressesInput.fill('10.0.0.1/32');

    await expect(privateKeyInput).toHaveValue('test-private-key-abc123');
    await expect(addressesInput).toHaveValue('10.0.0.1/32');
  });

  test('can fill server location preferences', async ({ page }) => {
    const countryInput = page.getByLabel('Country');
    const cityInput = page.getByLabel('City');
    const regionInput = page.getByLabel('Region');

    await countryInput.fill('United States');
    await cityInput.fill('New York');
    await regionInput.fill('New York');

    await expect(countryInput).toHaveValue('United States');
    await expect(cityInput).toHaveValue('New York');
    await expect(regionInput).toHaveValue('New York');
  });

  test('shows info box about VPN being optional', async ({ page }) => {
    await expect(page.getByText('VPN is optional')).toBeVisible();
    await expect(page.getByText('docker compose --profile vpn up')).toBeVisible();
  });

  test('switching protocol preserves provider selection', async ({ page }) => {
    // Select a specific provider
    await selectOption(page, 'VPN Provider', 'Mullvad');

    // Switch protocol
    await selectOption(page, 'Protocol', 'WireGuard');

    // Provider should still be mullvad
    await expect(page.getByLabel('VPN Provider')).toHaveText('Mullvad');
  });
});
