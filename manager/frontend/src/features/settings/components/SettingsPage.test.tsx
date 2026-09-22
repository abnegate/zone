import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import { TabsContent, TabsList, TabsTrigger } from '@zone/ui';
import { SettingsPage } from './SettingsPage';

describe('SettingsPage', () => {
  it('puts the title and the tabs on one page bar over a single scrolling body', () => {
    const { container } = render(
      <SettingsPage
        title="Organization Settings"
        value="members"
        onValueChange={() => undefined}
        tabs={
          <TabsList>
            <TabsTrigger value="ai">AI Settings</TabsTrigger>
            <TabsTrigger value="members">Members</TabsTrigger>
          </TabsList>
        }
      >
        <TabsContent value="members">Members body</TabsContent>
      </SettingsPage>
    );

    const page = container.firstElementChild as HTMLElement;
    expect(page.className).toContain('page--workspace');
    expect(page.className).toContain('settings-page');

    const bar = page.querySelector(':scope > .page-bar') as HTMLElement;
    expect(bar).not.toBeNull();
    expect(
      screen.getByRole('heading', { name: 'Organization Settings' }).closest('.page-bar')
    ).toBe(bar);
    expect(screen.getByRole('tab', { name: 'Members' }).closest('.page-bar')).toBe(bar);

    const body = page.querySelector(':scope > .page-body') as HTMLElement;
    expect(body).not.toBeNull();
    expect(body.querySelector('.page-container.settings-body')).not.toBeNull();
    expect(screen.getByText('Members body').closest('.page-body')).toBe(body);
  });

  it('renders without tabs while a page is still loading', () => {
    render(
      <SettingsPage title="Workspace Settings">
        <div className="loading-state">Loading theme settings...</div>
      </SettingsPage>
    );
    expect(screen.getByRole('heading', { name: 'Workspace Settings' })).toBeInTheDocument();
    expect(screen.queryByRole('tab')).toBeNull();
    expect(screen.getByText('Loading theme settings...')).toBeInTheDocument();
  });
});
