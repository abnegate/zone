import { describe, expect, it } from 'bun:test';
import { OPEN_RULES } from './draft';
import { ORGANIZATION_RULES, WORKSPACE_RULES } from './fixtures';
import { PolicyField } from './schemas';
import { PolicySource, sourceOf } from './source';

describe('sourceOf', () => {
  it('credits the workspace when it sets the field', () => {
    expect(sourceOf(PolicyField.Providers, ORGANIZATION_RULES, WORKSPACE_RULES)).toBe(
      PolicySource.Workspace
    );
    expect(sourceOf(PolicyField.Ceiling, ORGANIZATION_RULES, WORKSPACE_RULES)).toBe(
      PolicySource.Workspace
    );
  });

  it('credits the organization when only it sets the field', () => {
    expect(sourceOf(PolicyField.Models, ORGANIZATION_RULES, WORKSPACE_RULES)).toBe(
      PolicySource.Organization
    );
    expect(sourceOf(PolicyField.Ceiling, ORGANIZATION_RULES, OPEN_RULES)).toBe(
      PolicySource.Organization
    );
  });

  it('reports no restriction when neither layer sets the field', () => {
    expect(sourceOf(PolicyField.McpServers, ORGANIZATION_RULES, WORKSPACE_RULES)).toBe(
      PolicySource.None
    );
  });
});
