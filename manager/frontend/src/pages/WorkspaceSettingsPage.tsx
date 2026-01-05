import { type FormEvent, useCallback, useEffect, useState } from 'react';
import { client } from '../api/client';
import { Button } from '../components';
import { useAuth } from '../context/AuthContext';
import { useTheme } from '../context/ThemeContext';
import type {
  BorderRadius,
  FontFamily,
  UpdateWorkspaceThemeRequest,
  WorkspaceTheme,
} from '../types';
import './WorkspaceSettingsPage.css';

const DEFAULT_ORG_ID = '00000000-0000-0000-0000-000000000001';
const DEFAULT_WS_ID = '00000000-0000-0000-0000-000000000001';

const fontOptions: { value: FontFamily; label: string }[] = [
  { value: 'system', label: 'System Default' },
  { value: 'inter', label: 'Inter' },
  { value: 'roboto', label: 'Roboto' },
  { value: 'open-sans', label: 'Open Sans' },
  { value: 'lato', label: 'Lato' },
  { value: 'nunito', label: 'Nunito' },
];

const radiusOptions: { value: BorderRadius; label: string }[] = [
  { value: 'none', label: 'None' },
  { value: 'small', label: 'Small' },
  { value: 'medium', label: 'Medium' },
  { value: 'large', label: 'Large' },
];

export default function WorkspaceSettingsPage() {
  const { isAuthenticated } = useAuth();
  const { workspaceTheme, setWorkspaceTheme } = useTheme();

  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  // Form state
  const [primaryColorLight, setPrimaryColorLight] = useState('#3b82f6');
  const [secondaryColorLight, setSecondaryColorLight] = useState('#6366f1');
  const [primaryColorDark, setPrimaryColorDark] = useState('#3b82f6');
  const [secondaryColorDark, setSecondaryColorDark] = useState('#6366f1');
  const [fontFamily, setFontFamily] = useState<FontFamily>('system');
  const [fontSize, setFontSize] = useState('16');
  const [borderRadius, setBorderRadius] = useState<BorderRadius>('medium');

  // Load current theme
  const loadTheme = useCallback(async () => {
    if (!isAuthenticated) return;
    setLoading(true);
    setError(null);
    try {
      const theme = await client.getWorkspaceTheme(DEFAULT_ORG_ID, DEFAULT_WS_ID);
      applyThemeToForm(theme);
      setWorkspaceTheme(theme);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load theme');
    } finally {
      setLoading(false);
    }
  }, [isAuthenticated, setWorkspaceTheme]);

  const applyThemeToForm = (theme: WorkspaceTheme) => {
    setPrimaryColorLight(theme.primary_color_light);
    setSecondaryColorLight(theme.secondary_color_light);
    setPrimaryColorDark(theme.primary_color_dark);
    setSecondaryColorDark(theme.secondary_color_dark);
    setFontFamily(theme.font_family);
    setFontSize(theme.font_size_base.replace('px', ''));
    setBorderRadius(theme.border_radius);
  };

  useEffect(() => {
    loadTheme();
  }, [loadTheme]);

  // Live preview - apply changes to workspaceTheme as user edits
  useEffect(() => {
    if (loading) return;

    const previewTheme: WorkspaceTheme = {
      id: workspaceTheme?.id || '',
      workspace_id: workspaceTheme?.workspace_id || DEFAULT_WS_ID,
      primary_color_light: primaryColorLight,
      secondary_color_light: secondaryColorLight,
      primary_color_dark: primaryColorDark,
      secondary_color_dark: secondaryColorDark,
      font_family: fontFamily,
      font_size_base: `${fontSize}px`,
      border_radius: borderRadius,
      created_at: workspaceTheme?.created_at || '',
      updated_at: workspaceTheme?.updated_at || '',
    };
    setWorkspaceTheme(previewTheme);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- Intentionally exclude created_at/updated_at to prevent infinite loops after save
  }, [
    primaryColorLight,
    secondaryColorLight,
    primaryColorDark,
    secondaryColorDark,
    fontFamily,
    fontSize,
    borderRadius,
    loading,
    setWorkspaceTheme,
    workspaceTheme?.id,
    workspaceTheme?.workspace_id,
  ]);

  const handleSave = async (e: FormEvent) => {
    e.preventDefault();
    if (!isAuthenticated) return;

    setSaving(true);
    setError(null);
    setSuccess(null);

    try {
      const request: UpdateWorkspaceThemeRequest = {
        primary_color_light: primaryColorLight,
        secondary_color_light: secondaryColorLight,
        primary_color_dark: primaryColorDark,
        secondary_color_dark: secondaryColorDark,
        font_family: fontFamily,
        font_size_base: `${fontSize}px`,
        border_radius: borderRadius,
      };
      const theme = await client.updateWorkspaceTheme(DEFAULT_ORG_ID, DEFAULT_WS_ID, request);
      setWorkspaceTheme(theme);
      setSuccess('Theme saved successfully');
      setTimeout(() => setSuccess(null), 3000);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save theme');
    } finally {
      setSaving(false);
    }
  };

  const handleReset = async () => {
    if (!isAuthenticated) return;

    setSaving(true);
    setError(null);
    setSuccess(null);

    try {
      const theme = await client.resetWorkspaceTheme(DEFAULT_ORG_ID, DEFAULT_WS_ID);
      applyThemeToForm(theme);
      setWorkspaceTheme(theme);
      setSuccess('Theme reset to defaults');
      setTimeout(() => setSuccess(null), 3000);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to reset theme');
    } finally {
      setSaving(false);
    }
  };

  if (loading) {
    return (
      <div className="page-container">
        <div className="page-header">
          <h1 className="page-title">Workspace Settings</h1>
        </div>
        <div className="loading-state">Loading theme settings...</div>
      </div>
    );
  }

  return (
    <div className="page-container">
      <div className="page-header">
        <h1 className="page-title">Workspace Settings</h1>
      </div>

      {error && <div className="alert alert-error">{error}</div>}
      {success && <div className="alert alert-success">{success}</div>}

      <form onSubmit={handleSave} className="settings-form">
        <section className="settings-section">
          <h2 className="section-title">Theme Configuration</h2>

          <div className="settings-grid">
            {/* Light Mode Colors */}
            <div className="settings-card">
              <h3 className="card-title">Light Mode Colors</h3>
              <div className="form-group">
                <label htmlFor="primary-light">Primary Color</label>
                <div className="color-input-wrapper">
                  <input
                    type="color"
                    id="primary-light"
                    value={primaryColorLight}
                    onChange={(e) => setPrimaryColorLight(e.target.value)}
                  />
                  <input
                    type="text"
                    value={primaryColorLight}
                    onChange={(e) => setPrimaryColorLight(e.target.value)}
                    pattern="^#[0-9A-Fa-f]{6}$"
                    className="color-text-input"
                  />
                </div>
              </div>
              <div className="form-group">
                <label htmlFor="secondary-light">Secondary Color</label>
                <div className="color-input-wrapper">
                  <input
                    type="color"
                    id="secondary-light"
                    value={secondaryColorLight}
                    onChange={(e) => setSecondaryColorLight(e.target.value)}
                  />
                  <input
                    type="text"
                    value={secondaryColorLight}
                    onChange={(e) => setSecondaryColorLight(e.target.value)}
                    pattern="^#[0-9A-Fa-f]{6}$"
                    className="color-text-input"
                  />
                </div>
              </div>
            </div>

            {/* Dark Mode Colors */}
            <div className="settings-card">
              <h3 className="card-title">Dark Mode Colors</h3>
              <div className="form-group">
                <label htmlFor="primary-dark">Primary Color</label>
                <div className="color-input-wrapper">
                  <input
                    type="color"
                    id="primary-dark"
                    value={primaryColorDark}
                    onChange={(e) => setPrimaryColorDark(e.target.value)}
                  />
                  <input
                    type="text"
                    value={primaryColorDark}
                    onChange={(e) => setPrimaryColorDark(e.target.value)}
                    pattern="^#[0-9A-Fa-f]{6}$"
                    className="color-text-input"
                  />
                </div>
              </div>
              <div className="form-group">
                <label htmlFor="secondary-dark">Secondary Color</label>
                <div className="color-input-wrapper">
                  <input
                    type="color"
                    id="secondary-dark"
                    value={secondaryColorDark}
                    onChange={(e) => setSecondaryColorDark(e.target.value)}
                  />
                  <input
                    type="text"
                    value={secondaryColorDark}
                    onChange={(e) => setSecondaryColorDark(e.target.value)}
                    pattern="^#[0-9A-Fa-f]{6}$"
                    className="color-text-input"
                  />
                </div>
              </div>
            </div>
          </div>

          {/* Typography */}
          <div className="settings-card">
            <h3 className="card-title">Typography</h3>
            <div className="settings-row">
              <div className="form-group">
                <label htmlFor="font-family">Font Family</label>
                <select
                  id="font-family"
                  value={fontFamily}
                  onChange={(e) => setFontFamily(e.target.value as FontFamily)}
                  className="form-select"
                >
                  {fontOptions.map((opt) => (
                    <option key={opt.value} value={opt.value}>
                      {opt.label}
                    </option>
                  ))}
                </select>
              </div>
              <div className="form-group">
                <label htmlFor="font-size">Base Font Size</label>
                <div className="slider-input-wrapper">
                  <input
                    type="range"
                    id="font-size"
                    min="12"
                    max="20"
                    value={fontSize}
                    onChange={(e) => setFontSize(e.target.value)}
                    className="form-slider"
                  />
                  <span className="slider-value">{fontSize}px</span>
                </div>
              </div>
            </div>
          </div>

          {/* Appearance */}
          <div className="settings-card">
            <h3 className="card-title">Appearance</h3>
            <div className="form-group">
              <label>Corner Radius</label>
              <div className="radio-group">
                {radiusOptions.map((opt) => (
                  <label key={opt.value} className="radio-option">
                    <input
                      type="radio"
                      name="border-radius"
                      value={opt.value}
                      checked={borderRadius === opt.value}
                      onChange={() => setBorderRadius(opt.value)}
                    />
                    <span className="radio-label">{opt.label}</span>
                  </label>
                ))}
              </div>
            </div>
          </div>

          {/* Preview */}
          <div className="settings-card">
            <h3 className="card-title">Preview</h3>
            <div className="preview-box">
              <p className="preview-text">
                This is a preview of your theme settings. Changes are applied live.
              </p>
              <div className="preview-buttons">
                <Button type="button" variant="primary">
                  Primary Button
                </Button>
                <Button type="button" variant="secondary">
                  Secondary Button
                </Button>
              </div>
              <div className="preview-card">
                <strong>Sample Card</strong>
                <p>This card demonstrates the corner radius and colors.</p>
              </div>
            </div>
          </div>
        </section>

        {/* Actions */}
        <div className="settings-actions">
          <Button type="button" onClick={handleReset} disabled={saving} variant="secondary">
            Reset to Defaults
          </Button>
          <Button type="submit" loading={saving} variant="primary">
            {saving ? 'Saving...' : 'Save Changes'}
          </Button>
        </div>
      </form>
    </div>
  );
}
