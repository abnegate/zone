import { test, expect } from '@playwright/test';

test.describe('VPN Configuration', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    // Navigate to VPN step (step 6)
    await page.click('.stepper-item:nth-child(6) .stepper-button');
    await expect(page.locator('h2')).toContainText('VPN Configuration');
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
    const protocolSelect = page.getByLabel('Protocol');
    await protocolSelect.selectOption('wireguard');

    // Should now show WireGuard-specific fields
    await expect(page.getByLabel('Private Key')).toBeVisible();
    await expect(page.getByLabel('Addresses')).toBeVisible();

    // OpenVPN fields should be hidden
    await expect(page.getByLabel('Username')).not.toBeVisible();
    await expect(page.getByLabel('Password')).not.toBeVisible();
  });

  test('can select different VPN providers', async ({ page }) => {
    const providerSelect = page.getByLabel('VPN Provider');

    // Test selecting each provider
    const providers = ['surfshark', 'nordvpn', 'expressvpn', 'protonvpn', 'mullvad'];

    for (const provider of providers) {
      await providerSelect.selectOption(provider);
      await expect(providerSelect).toHaveValue(provider);
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
    await page.getByLabel('Protocol').selectOption('wireguard');

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
    await page.getByLabel('VPN Provider').selectOption('mullvad');

    // Switch protocol
    await page.getByLabel('Protocol').selectOption('wireguard');

    // Provider should still be mullvad
    await expect(page.getByLabel('VPN Provider')).toHaveValue('mullvad');
  });
});
