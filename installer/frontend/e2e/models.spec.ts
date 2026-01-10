import { test, expect } from '@playwright/test';

test.describe('AI Provider Configuration', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    // Navigate to Models step (step 3)
    await page.click('.stepper-item:nth-child(3) .stepper-button');
    await expect(page.locator('h2')).toContainText('AI Provider Configuration');
  });

  test('displays default self-hosted provider', async ({ page }) => {
    const providerSelect = page.locator('select').first();
    await expect(providerSelect).toHaveValue('self_hosted');
  });

  test('shows LiteLLM configuration for self-hosted', async ({ page }) => {
    await expect(page.getByText('LiteLLM Configuration')).toBeVisible();
    await expect(page.getByLabel('LiteLLM Host')).toBeVisible();
    await expect(page.getByLabel('LiteLLM API Key (Optional)')).toBeVisible();
  });

  test('switches to OpenAI provider and shows API key field', async ({ page }) => {
    await page.selectOption('select', 'openai');

    await expect(page.getByText('OpenAI Configuration')).toBeVisible();
    await expect(page.getByLabel('OpenAI API Key')).toBeVisible();
    await expect(page.getByLabel('Base URL (Optional)')).toBeVisible();
  });

  test('switches to Anthropic provider and shows warning about embeddings', async ({ page }) => {
    await page.selectOption('select', 'anthropic');

    await expect(page.getByText('Anthropic Configuration')).toBeVisible();
    await expect(page.getByLabel('Anthropic API Key')).toBeVisible();
    await expect(page.getByText('Anthropic does not provide embedding models')).toBeVisible();
  });

  test('switches to AWS Bedrock and shows region selector', async ({ page }) => {
    await page.selectOption('select', 'bedrock');

    await expect(page.getByText('AWS Bedrock Configuration')).toBeVisible();
    await expect(page.getByLabel('AWS Region')).toBeVisible();
    await expect(page.getByLabel('Use IAM Role')).toBeVisible();
  });

  test('Bedrock shows credential fields when IAM role unchecked', async ({ page }) => {
    await page.selectOption('select', 'bedrock');

    // By default, IAM role should be unchecked, showing credential fields
    const iamCheckbox = page.getByLabel('Use IAM Role');
    const isChecked = await iamCheckbox.isChecked();

    if (isChecked) {
      await iamCheckbox.click();
    }

    await expect(page.getByLabel('AWS Access Key ID')).toBeVisible();
    await expect(page.getByLabel('AWS Secret Access Key')).toBeVisible();
  });

  test('Bedrock hides credential fields when IAM role checked', async ({ page }) => {
    await page.selectOption('select', 'bedrock');

    const iamCheckbox = page.getByLabel('Use IAM Role');
    if (!(await iamCheckbox.isChecked())) {
      await iamCheckbox.click();
    }

    await expect(page.getByLabel('AWS Access Key ID')).not.toBeVisible();
    await expect(page.getByLabel('AWS Secret Access Key')).not.toBeVisible();
  });

  test('displays model selection dropdowns', async ({ page }) => {
    await expect(page.getByLabel('Fast Model')).toBeVisible();
    await expect(page.getByLabel('Reasoning Model')).toBeVisible();
    await expect(page.getByLabel('Embedding Model')).toBeVisible();
  });

  test('model options change when provider changes', async ({ page }) => {
    // Get initial fast model value (self-hosted)
    const fastModelSelect = page.getByLabel('Fast Model');
    const initialValue = await fastModelSelect.inputValue();
    expect(initialValue).toContain('llama');

    // Switch to OpenAI
    await page.selectOption('select', 'openai');

    // Fast model should now show GPT models
    const newValue = await fastModelSelect.inputValue();
    expect(newValue).toContain('gpt');
  });

  test('shows info box about model downloads for self-hosted', async ({ page }) => {
    await expect(page.getByText('Models will download on first start')).toBeVisible();
  });

  test('shows API billing info for OpenAI', async ({ page }) => {
    await page.selectOption('select', 'openai');
    await expect(page.getByText('API usage will be billed')).toBeVisible();
  });

  test('shows Bedrock billing info', async ({ page }) => {
    await page.selectOption('select', 'bedrock');
    await expect(page.getByText('AWS Bedrock usage is billed')).toBeVisible();
  });

  test('can fill LiteLLM host for self-hosted', async ({ page }) => {
    const hostInput = page.getByLabel('LiteLLM Host');
    await hostInput.clear();
    await hostInput.fill('http://localhost:11434');

    await expect(hostInput).toHaveValue('http://localhost:11434');
  });

  test('can fill OpenAI API key', async ({ page }) => {
    await page.selectOption('select', 'openai');

    const apiKeyInput = page.getByLabel('OpenAI API Key');
    await apiKeyInput.fill('sk-test-key-12345');

    await expect(apiKeyInput).toHaveValue('sk-test-key-12345');
  });

  test('can select different AWS region', async ({ page }) => {
    await page.selectOption('select', 'bedrock');

    const regionSelect = page.getByLabel('AWS Region');
    await regionSelect.selectOption('eu-west-1');

    await expect(regionSelect).toHaveValue('eu-west-1');
  });

  test('Anthropic shows external embedding model input', async ({ page }) => {
    await page.selectOption('select', 'anthropic');

    // Anthropic has no embedding models, so it shows an input instead of select
    await expect(page.getByLabel('Embedding Model (External)')).toBeVisible();
  });
});
