import { Button, Select } from '@zone/ui';
import {
  type CSSProperties,
  type FormEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { client } from '../../../../api/client';
import { useTheme, workspaceThemeProperties } from '../../../../shared/context/ThemeContext';
import { useWorkspace } from '../../../../shared/context/WorkspaceContext';
import { useAuth } from '../../../auth';
import { UpdateWorkspaceThemeRequestSchema } from '../schemas';
import type {
  BorderRadius,
  FontFamily,
  UpdateWorkspaceThemeRequest,
  WorkspaceTheme,
} from '../types';
import ColorField from './ColorField';

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

const DEFAULT_PRIMARY_LIGHT = '#0011d9';
const DEFAULT_PRIMARY_DARK = '#00f3ff';
const DEFAULT_SECONDARY = '#ecf9ff';
const DEFAULT_FONT: FontFamily = 'nunito';
const DEFAULT_FONT_SIZE = '16';
const DEFAULT_RADIUS: BorderRadius = 'large';
const FLASH_DURATION = 3000;

export default function GeneralSection({ editable }: { editable: boolean }) {
  const { isAuthenticated } = useAuth();
  const {
    theme,
    workspaceTheme,
    workspaceThemeLoading,
    workspaceThemeError,
    setWorkspaceTheme,
    previewWorkspaceTheme,
  } = useTheme();
  const { currentOrganization, currentWorkspace } = useWorkspace();
  const orgId = currentOrganization?.id ?? null;
  const workspaceId = currentWorkspace?.id ?? null;

  const [dirty, setDirty] = useState(false);
  const edited = useRef(false);
  const touched = useRef(new Set<keyof UpdateWorkspaceThemeRequest>());
  const scope = `${orgId}/${workspaceId}`;
  const currentScope = useRef<string | null>(scope);
  currentScope.current = scope;
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  const [primaryColorLight, setPrimaryColorLight] = useState(DEFAULT_PRIMARY_LIGHT);
  const [secondaryColorLight, setSecondaryColorLight] = useState(DEFAULT_SECONDARY);
  const [primaryColorDark, setPrimaryColorDark] = useState(DEFAULT_PRIMARY_DARK);
  const [secondaryColorDark, setSecondaryColorDark] = useState(DEFAULT_SECONDARY);
  const [fontFamily, setFontFamily] = useState<FontFamily | null>(DEFAULT_FONT);
  const [fontSize, setFontSize] = useState(DEFAULT_FONT_SIZE);
  const [borderRadius, setBorderRadius] = useState<BorderRadius | null>(DEFAULT_RADIUS);

  const applyThemeToForm = useCallback((saved: WorkspaceTheme | null): void => {
    setPrimaryColorLight(saved?.primary_color_light ?? DEFAULT_PRIMARY_LIGHT);
    setSecondaryColorLight(saved?.secondary_color_light ?? DEFAULT_SECONDARY);
    setPrimaryColorDark(saved?.primary_color_dark ?? DEFAULT_PRIMARY_DARK);
    setSecondaryColorDark(saved?.secondary_color_dark ?? DEFAULT_SECONDARY);
    setFontFamily(saved ? (saved.font_family ?? null) : DEFAULT_FONT);
    setFontSize((saved?.font_size_base ?? `${DEFAULT_FONT_SIZE}px`).replace('px', ''));
    setBorderRadius(saved ? (saved.border_radius ?? null) : DEFAULT_RADIUS);
  }, []);

  const clearDraft = useCallback((): void => {
    edited.current = false;
    touched.current.clear();
    setDirty(false);
    previewWorkspaceTheme(null);
  }, [previewWorkspaceTheme]);

  useEffect(() => {
    applyThemeToForm(workspaceTheme?.workspace_id === workspaceId ? workspaceTheme : null);
    clearDraft();
  }, [workspaceTheme, workspaceId, applyThemeToForm, clearDraft]);

  useEffect(() => () => previewWorkspaceTheme(null), [previewWorkspaceTheme]);

  useEffect(() => {
    currentScope.current = scope;
    setSaving(false);
    setError(null);
    setSuccess(null);
    return () => {
      currentScope.current = null;
    };
  }, [scope]);

  const createThemeRequest = useCallback((): Required<UpdateWorkspaceThemeRequest> => {
    const saved = workspaceTheme?.workspace_id === workspaceId ? workspaceTheme : null;
    const preserve = <T extends string>(
      field: keyof UpdateWorkspaceThemeRequest,
      value: T | null,
      previous: T | null | undefined
    ): T | null => (touched.current.has(field) ? value : (previous ?? null));
    return {
      primary_color_light: preserve(
        'primary_color_light',
        primaryColorLight,
        saved?.primary_color_light
      ),
      secondary_color_light: preserve(
        'secondary_color_light',
        secondaryColorLight,
        saved?.secondary_color_light
      ),
      primary_color_dark: preserve(
        'primary_color_dark',
        primaryColorDark,
        saved?.primary_color_dark
      ),
      secondary_color_dark: preserve(
        'secondary_color_dark',
        secondaryColorDark,
        saved?.secondary_color_dark
      ),
      font_family: preserve('font_family', fontFamily, saved?.font_family),
      font_size_base: preserve('font_size_base', `${fontSize}px`, saved?.font_size_base),
      border_radius: preserve('border_radius', borderRadius, saved?.border_radius),
    };
  }, [
    workspaceTheme,
    workspaceId,
    primaryColorLight,
    secondaryColorLight,
    primaryColorDark,
    secondaryColorDark,
    fontFamily,
    fontSize,
    borderRadius,
  ]);

  const previewStyle = useMemo(() => {
    const style: Record<string, string> = {};
    const draft: WorkspaceTheme = {
      workspace_id: workspaceId ?? '',
      primary_color_light: primaryColorLight,
      secondary_color_light: secondaryColorLight,
      primary_color_dark: primaryColorDark,
      secondary_color_dark: secondaryColorDark,
      font_family: fontFamily,
      font_size_base: `${fontSize}px`,
      border_radius: borderRadius,
      created_at: '',
      updated_at: '',
    };
    for (const [name, value] of workspaceThemeProperties(draft, theme)) {
      style[name === 'font-size' ? 'fontSize' : name] = value;
    }
    return style as CSSProperties;
  }, [
    workspaceId,
    primaryColorLight,
    secondaryColorLight,
    primaryColorDark,
    secondaryColorDark,
    fontFamily,
    fontSize,
    borderRadius,
    theme,
  ]);

  useEffect(() => {
    if (!dirty || !edited.current || workspaceThemeLoading || !workspaceId) return;
    const request = createThemeRequest();
    const result = UpdateWorkspaceThemeRequestSchema.safeParse(request);
    if (!result.success) return;
    previewWorkspaceTheme({
      workspace_id: workspaceId,
      ...request,
      created_at: '',
      updated_at: '',
    });
  }, [dirty, workspaceThemeLoading, workspaceId, createThemeRequest, previewWorkspaceTheme]);

  const flash = (message: string) => {
    setSuccess(message);
    setTimeout(() => {
      if (currentScope.current === scope) setSuccess(null);
    }, FLASH_DURATION);
  };

  const handleSave = async (event: FormEvent): Promise<void> => {
    event.preventDefault();
    if (!editable || !isAuthenticated || !orgId || !workspaceId) return;

    setSaving(true);
    setError(null);
    setSuccess(null);

    try {
      const saved = await client.updateWorkspaceTheme(
        orgId,
        workspaceId,
        UpdateWorkspaceThemeRequestSchema.parse(createThemeRequest())
      );
      if (currentScope.current !== scope) return;
      setWorkspaceTheme(saved);
      applyThemeToForm(saved);
      clearDraft();
      flash('Settings saved successfully');
    } catch (failure) {
      if (currentScope.current === scope)
        setError(failure instanceof Error ? failure.message : 'Failed to save settings');
    } finally {
      if (currentScope.current === scope) setSaving(false);
    }
  };

  const handleReset = async (): Promise<void> => {
    if (!editable || !isAuthenticated || !orgId || !workspaceId) return;

    setSaving(true);
    setError(null);
    setSuccess(null);

    try {
      await client.resetWorkspaceTheme(orgId, workspaceId);
      if (currentScope.current !== scope) return;
      setWorkspaceTheme(null);
      applyThemeToForm(null);
      clearDraft();
      flash('Settings reset to defaults');
    } catch (failure) {
      if (currentScope.current === scope)
        setError(failure instanceof Error ? failure.message : 'Failed to reset settings');
    } finally {
      if (currentScope.current === scope) setSaving(false);
    }
  };

  const colorField = (
    field: keyof UpdateWorkspaceThemeRequest,
    set: (value: string) => void
  ): ((value: string) => void) => {
    return (value) => {
      touched.current.add(field);
      set(value);
    };
  };

  if (workspaceThemeLoading) {
    return <div className="loading-state">Loading theme settings...</div>;
  }

  if (!orgId || !workspaceId) {
    return (
      <div className="alert alert-error">
        No workspace selected. Please select or create a workspace first.
      </div>
    );
  }

  return (
    <>
      {(error || workspaceThemeError) && (
        <div className="alert alert-error">{error || workspaceThemeError}</div>
      )}
      {success && <div className="alert alert-success">{success}</div>}

      <form
        onSubmit={handleSave}
        onChange={() => {
          edited.current = true;
          setDirty(true);
        }}
        className="settings-form"
      >
        <div className="section-row">
          <div className="section-row-copy">
            <h2 className="section-title">Theme Configuration</h2>
            <p className="section-description">
              Changes preview live and apply to everyone in this workspace once saved.
            </p>
          </div>
        </div>

        {!editable && <p className="form-hint">Viewers can't change the workspace theme.</p>}

        <fieldset disabled={!editable} className="settings-form">
          <div className="settings-card">
            <h3 className="card-title">Colors</h3>
            <div className="theme-groups">
              <div className="theme-group">
                <h4 className="settings-eyebrow">Light Mode Colors</h4>
                <div className="form-grid">
                  <ColorField
                    id="primary-light"
                    label="Primary Color"
                    value={primaryColorLight}
                    onChange={colorField('primary_color_light', setPrimaryColorLight)}
                  />
                  <ColorField
                    id="secondary-light"
                    label="Secondary Color"
                    value={secondaryColorLight}
                    onChange={colorField('secondary_color_light', setSecondaryColorLight)}
                  />
                </div>
              </div>
              <div className="theme-group">
                <h4 className="settings-eyebrow">Dark Mode Colors</h4>
                <div className="form-grid">
                  <ColorField
                    id="primary-dark"
                    label="Primary Color"
                    value={primaryColorDark}
                    onChange={colorField('primary_color_dark', setPrimaryColorDark)}
                  />
                  <ColorField
                    id="secondary-dark"
                    label="Secondary Color"
                    value={secondaryColorDark}
                    onChange={colorField('secondary_color_dark', setSecondaryColorDark)}
                  />
                </div>
              </div>
            </div>
          </div>

          <div className="settings-card">
            <h3 className="card-title">Typography &amp; shape</h3>
            <div className="form-grid">
              <Select
                id="font-family"
                label="Font Family"
                value={fontFamily ?? ''}
                disabled={!editable}
                onValueChange={(next) => {
                  touched.current.add('font_family');
                  edited.current = true;
                  setDirty(true);
                  setFontFamily(next as FontFamily);
                }}
                wrapperClassName="form-group"
                options={[{ value: '', label: 'App Default', disabled: true }, ...fontOptions]}
              />
              <div className="form-group">
                <label htmlFor="font-size">Base Font Size</label>
                <div className="slider-input-wrapper">
                  <input
                    type="range"
                    id="font-size"
                    min="12"
                    max="20"
                    value={fontSize}
                    onChange={(event) => {
                      touched.current.add('font_size_base');
                      setFontSize(event.target.value);
                    }}
                    className="form-slider"
                  />
                  <span className="slider-value">{fontSize}px</span>
                </div>
              </div>
              <div className="form-group form-group--full">
                <span className="form-label">Corner Radius</span>
                <div className="radio-group">
                  {borderRadius === null && (
                    <label className="radio-option">
                      <input type="radio" name="border-radius" value="" checked disabled />
                      <span className="radio-label">App Default</span>
                    </label>
                  )}
                  {radiusOptions.map((option) => (
                    <label key={option.value} className="radio-option">
                      <input
                        type="radio"
                        name="border-radius"
                        value={option.value}
                        checked={borderRadius === option.value}
                        onChange={() => {
                          touched.current.add('border_radius');
                          setBorderRadius(option.value);
                        }}
                      />
                      <span className="radio-label">{option.label}</span>
                    </label>
                  ))}
                </div>
              </div>
            </div>
          </div>
        </fieldset>

        <div className="preview-box" style={previewStyle}>
          <h3 className="settings-eyebrow">Preview</h3>
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

        {editable && (
          <div className="settings-actions">
            <Button type="button" variant="ghost" size="sm" onClick={handleReset} disabled={saving}>
              Reset to Defaults
            </Button>
            <Button type="submit" loading={saving}>
              {saving ? 'Saving...' : 'Save Changes'}
            </Button>
          </div>
        )}
      </form>
    </>
  );
}
