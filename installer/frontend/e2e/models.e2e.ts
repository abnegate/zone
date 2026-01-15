import { test, expect } from '@playwright/test';
import { selectOption } from './helpers';

test.describe('AI Provider Configuration', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    // Navigate to Models step (step 3)
    await page.click('[data-step="3"]');
    await expect(page.getByRole('heading', { name: 'AI Provider Configuration' })).toBeVisible();
  });

  test('displays default self-hosted provider', async ({ page }) => {
    const providerSelect = page.getByLabel('AI Provider');
    await expect(providerSelect).toHaveText(/Self-Hosted/i);
  });

  test('shows LiteLLM configuration for self-hosted', async ({ page }) => {
    await expect(page.getByText('LiteLLM Configuration')).toBeVisible();
    await expect(page.getByLabel('LiteLLM Host')).toBeVisible();
    await expect(page.getByLabel('LiteLLM API Key (Optional)')).toBeVisible();
  });

  test('switches to OpenAI provider and shows API key field', async ({ page }) => {
    await selectOption(page, 'AI Provider', 'OpenAI');

    await expect(page.getByText('OpenAI Configuration')).toBeVisible();
    await expect(page.getByLabel('OpenAI API Key')).toBeVisible();
    await expect(page.getByLabel('Base URL (Optional)')).toBeVisible();
  });

  test('switches to Anthropic provider and shows warning about embeddings', async ({ page }) => {
    await selectOption(page, 'AI Provider', 'Anthropic');

    await expect(page.getByText('Anthropic Configuration')).toBeVisible();
    await expect(page.getByLabel('Anthropic API Key')).toBeVisible();
    await expect(page.getByText('Anthropic does not provide embedding models')).toBeVisible();
  });

  test('switches to AWS Bedrock and shows region selector', async ({ page }) => {
    await selectOption(page, 'AI Provider', 'AWS Bedrock');

    await expect(page.getByText('AWS Bedrock Configuration')).toBeVisible();
    await expect(page.getByLabel('AWS Region')).toBeVisible();
    await expect(page.getByLabel('Use IAM Role')).toBeVisible();
  });

  test('Bedrock shows credential fields when IAM role unchecked', async ({ page }) => {
    await selectOption(page, 'AI Provider', 'AWS Bedrock');

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
    await selectOption(page, 'AI Provider', 'AWS Bedrock');

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
    const fastModelSelect = page.getByLabel('Fast Model');
    await expect(fastModelSelect).toHaveText(/llama/i);

    await selectOption(page, 'AI Provider', 'OpenAI');

    await fastModelSelect.click();
    await expect(page.getByRole('option', { name: /GPT-4o Mini/i })).toBeVisible();
  });

  test('shows info box about model downloads for self-hosted', async ({ page }) => {
    await expect(page.getByText('Models will download on first start')).toBeVisible();
  });

  test('shows API billing info for OpenAI', async ({ page }) => {
    await selectOption(page, 'AI Provider', 'OpenAI');
    await expect(page.getByText('API usage will be billed')).toBeVisible();
  });

  test('shows Bedrock billing info', async ({ page }) => {
    await selectOption(page, 'AI Provider', 'AWS Bedrock');
    await expect(page.getByText('AWS Bedrock usage is billed')).toBeVisible();
  });

  test('can fill LiteLLM host for self-hosted', async ({ page }) => {
    const hostInput = page.getByLabel('LiteLLM Host');
    await hostInput.clear();
    await hostInput.fill('http://localhost:11434');

    await expect(hostInput).toHaveValue('http://localhost:11434');
  });

  test('can fill OpenAI API key', async ({ page }) => {
    await selectOption(page, 'AI Provider', 'OpenAI');

    const apiKeyInput = page.getByLabel('OpenAI API Key');
    await apiKeyInput.fill('sk-test-key-12345');

    await expect(apiKeyInput).toHaveValue('sk-test-key-12345');
  });

  test('can select different AWS region', async ({ page }) => {
    await selectOption(page, 'AI Provider', 'AWS Bedrock');

    const regionSelect = page.getByLabel('AWS Region');
    await selectOption(page, 'AWS Region', 'Europe (Ireland)');
    await expect(regionSelect).toHaveText('Europe (Ireland)');
  });

  test('Anthropic shows external embedding model input', async ({ page }) => {
    await selectOption(page, 'AI Provider', 'Anthropic');

    // Anthropic has no embedding models, so it shows an input instead of select
    await expect(page.getByLabel('Embedding Model (External)')).toBeVisible();
  });
});
