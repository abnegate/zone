import { Select } from '@zone/ui';
import { NavLink } from 'react-router-dom';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { GROUP_LABELS, type SectionDefinition, SettingsGroup, settingsPath } from './sections';
import './SettingsNav.css';

const GROUP_ORDER: readonly SettingsGroup[] = [
  SettingsGroup.Workspace,
  SettingsGroup.Organization,
  SettingsGroup.Server,
  SettingsGroup.You,
];

export default function SettingsNav({ sections }: { sections: SectionDefinition[] }) {
  const { currentOrganization, currentWorkspace, workspaces, setCurrentWorkspace } = useWorkspace();

  const context: Partial<Record<SettingsGroup, string>> = {
    [SettingsGroup.Workspace]: currentWorkspace?.name,
    [SettingsGroup.Organization]: currentOrganization?.name,
  };

  const switchable = workspaces.length > 1;

  const switchWorkspace = (workspaceId: string) => {
    const workspace = workspaces.find((candidate) => candidate.id === workspaceId);
    if (workspace) {
      setCurrentWorkspace(workspace);
    }
  };

  return (
    <nav aria-label="Settings" className="settings-nav">
      {GROUP_ORDER.map((group) => {
        const items = sections.filter((definition) => definition.group === group);
        if (items.length === 0) {
          return null;
        }
        const headingId = `settings-nav-${group}`;
        return (
          <div key={group} role="group" aria-labelledby={headingId} className="settings-nav-group">
            <h2 id={headingId} className="settings-nav-heading">
              {GROUP_LABELS[group]}
            </h2>
            {group === SettingsGroup.Workspace && switchable ? (
              <Select
                compact
                aria-label="Switch workspace"
                value={currentWorkspace?.id}
                options={workspaces.map((workspace) => ({
                  value: workspace.id,
                  label: workspace.name,
                }))}
                onValueChange={switchWorkspace}
                wrapperClassName="settings-nav-switcher"
              />
            ) : (
              context[group] && <p className="settings-nav-context">{context[group]}</p>
            )}
            <ul className="settings-nav-list">
              {items.map((definition) => (
                <li key={definition.section}>
                  <NavLink
                    to={settingsPath(definition.group, definition.section)}
                    className="settings-nav-item"
                  >
                    {definition.label}
                  </NavLink>
                </li>
              ))}
            </ul>
          </div>
        );
      })}
    </nav>
  );
}
