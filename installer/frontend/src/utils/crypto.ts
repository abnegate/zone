import type { InstallerConfig } from '../types';

const STORAGE_KEY = 'zone_installer_config';
const ENCRYPTION_KEY_NAME = 'zone_installer_key';

// Sensitive fields that should be encrypted before storage
export const SENSITIVE_FIELDS: (keyof InstallerConfig)[] = [
  'SECURITY_LITELLM_MASTER_KEY',
  'SECURITY_LITELLM_SALT_KEY',
  'SECURITY_SEARXNG_SECRET_KEY',
  'SECURITY_MANAGER_API_KEY',
  'POSTGRES_PASSWORD',
  'VPN_OPENVPN_PASSWORD',
  'VPN_WIREGUARD_PRIVATE_KEY',
  'MONITORING_GRAFANA_ADMIN_PASSWORD',
  'ALERT_SMTP_PASSWORD',
];

async function getOrCreateKey(): Promise<CryptoKey> {
  const storedKey = sessionStorage.getItem(ENCRYPTION_KEY_NAME);

  if (storedKey) {
    const keyData = JSON.parse(storedKey);
    return await crypto.subtle.importKey(
      'jwk',
      keyData,
      { name: 'AES-GCM', length: 256 },
      true,
      ['encrypt', 'decrypt']
    );
  }

  const key = await crypto.subtle.generateKey(
    { name: 'AES-GCM', length: 256 },
    true,
    ['encrypt', 'decrypt']
  );

  const exportedKey = await crypto.subtle.exportKey('jwk', key);
  sessionStorage.setItem(ENCRYPTION_KEY_NAME, JSON.stringify(exportedKey));

  return key;
}

async function encrypt(text: string, key: CryptoKey): Promise<string> {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const encoded = new TextEncoder().encode(text);

  const ciphertext = await crypto.subtle.encrypt(
    { name: 'AES-GCM', iv },
    key,
    encoded
  );

  const combined = new Uint8Array(iv.length + ciphertext.byteLength);
  combined.set(iv);
  combined.set(new Uint8Array(ciphertext), iv.length);

  return btoa(String.fromCharCode(...Array.from(combined)));
}

async function decrypt(data: string, key: CryptoKey): Promise<string> {
  const combined = new Uint8Array(
    atob(data).split('').map(c => c.charCodeAt(0))
  );

  const iv = combined.slice(0, 12);
  const ciphertext = combined.slice(12);

  const decrypted = await crypto.subtle.decrypt(
    { name: 'AES-GCM', iv },
    key,
    ciphertext
  );

  return new TextDecoder().decode(decrypted);
}

export async function saveConfig(config: InstallerConfig): Promise<void> {
  try {
    const key = await getOrCreateKey();
    const toStore: Record<string, string> = {};

    for (const [field, value] of Object.entries(config)) {
      if (SENSITIVE_FIELDS.includes(field as keyof InstallerConfig) && value) {
        toStore[field] = await encrypt(value, key);
        toStore[`${field}_encrypted`] = 'true';
      } else {
        toStore[field] = value;
      }
    }

    localStorage.setItem(STORAGE_KEY, JSON.stringify(toStore));
  } catch (error) {
    console.error('Failed to save config:', error);
  }
}

export async function loadConfig(): Promise<Partial<InstallerConfig> | null> {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (!stored) return null;

  try {
    const data = JSON.parse(stored);
    const key = await getOrCreateKey();
    const config: Record<string, string> = {};

    for (const [field, value] of Object.entries(data)) {
      if (field.endsWith('_encrypted')) continue;

      if (data[`${field}_encrypted`] === 'true') {
        try {
          config[field] = await decrypt(value as string, key);
        } catch {
          // If decryption fails (e.g., different session), use empty string
          config[field] = '';
        }
      } else {
        config[field] = value as string;
      }
    }

    return config as Partial<InstallerConfig>;
  } catch {
    return null;
  }
}

export function clearConfig(): void {
  localStorage.removeItem(STORAGE_KEY);
}
