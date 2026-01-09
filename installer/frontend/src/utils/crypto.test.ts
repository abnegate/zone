import { clearConfig, loadConfig, SENSITIVE_FIELDS } from './crypto';

describe('crypto utilities', () => {
  beforeEach(() => {
    localStorage.clear();
    sessionStorage.clear();
  });

  describe('SENSITIVE_FIELDS', () => {
    it('includes security-related fields', () => {
      expect(SENSITIVE_FIELDS).toContain('SECURITY_LITELLM_MASTER_KEY');
      expect(SENSITIVE_FIELDS).toContain('SECURITY_LITELLM_SALT_KEY');
      expect(SENSITIVE_FIELDS).toContain('SECURITY_SEARXNG_SECRET_KEY');
      expect(SENSITIVE_FIELDS).toContain('SECURITY_MANAGER_API_KEY');
      expect(SENSITIVE_FIELDS).toContain('POSTGRES_PASSWORD');
    });

    it('includes VPN credentials', () => {
      expect(SENSITIVE_FIELDS).toContain('VPN_OPENVPN_PASSWORD');
      expect(SENSITIVE_FIELDS).toContain('VPN_WIREGUARD_PRIVATE_KEY');
    });

    it('includes monitoring credentials', () => {
      expect(SENSITIVE_FIELDS).toContain('MONITORING_GRAFANA_ADMIN_PASSWORD');
      expect(SENSITIVE_FIELDS).toContain('ALERT_SMTP_PASSWORD');
    });

    it('includes AI provider credentials', () => {
      expect(SENSITIVE_FIELDS).toContain('AI_LITELLM_KEY');
      expect(SENSITIVE_FIELDS).toContain('AI_OPENAI_API_KEY');
      expect(SENSITIVE_FIELDS).toContain('AI_ANTHROPIC_API_KEY');
      expect(SENSITIVE_FIELDS).toContain('AI_BEDROCK_ACCESS_KEY');
      expect(SENSITIVE_FIELDS).toContain('AI_BEDROCK_SECRET_KEY');
    });

    it('has expected number of sensitive fields', () => {
      expect(SENSITIVE_FIELDS.length).toBe(14);
    });
  });

  describe('loadConfig', () => {
    it('returns null when no config is stored', async () => {
      const loaded = await loadConfig();
      expect(loaded).toBeNull();
    });

    it('returns null on invalid JSON in localStorage', async () => {
      localStorage.setItem('zone_installer_config', 'invalid-json');

      const loaded = await loadConfig();
      expect(loaded).toBeNull();
    });
  });

  describe('clearConfig', () => {
    it('removes config from localStorage', () => {
      localStorage.setItem('zone_installer_config', '{"test": "data"}');
      expect(localStorage.getItem('zone_installer_config')).toBeTruthy();

      clearConfig();

      expect(localStorage.getItem('zone_installer_config')).toBeNull();
    });

    it('does nothing if no config exists', () => {
      // Should not throw
      expect(() => clearConfig()).not.toThrow();
      expect(localStorage.getItem('zone_installer_config')).toBeNull();
    });
  });
});
