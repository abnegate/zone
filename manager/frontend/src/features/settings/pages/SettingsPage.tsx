import type { ReactElement, ReactNode } from 'react';
import { Link, Navigate, useLocation, useParams } from 'react-router-dom';
import PageBar from '../../../shared/components/PageBar/PageBar';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { useMediaQuery } from '../../../shared/hooks';
import { legacyWorkspacePath, relocate } from '../navigation/legacy';
import SettingsNav from '../navigation/SettingsNav';
import {
  COMPACT_QUERY,
  findSection,
  SETTINGS_ROOT,
  SETTINGS_TITLE,
  type SectionDefinition,
  settingsPath,
  visibleSections,
} from '../navigation/sections';
import { useSettingsAccess } from '../navigation/useSettingsAccess';
import SettingsContent from './SettingsContent';
import './SettingsPage.css';

function firstPath(sections: SectionDefinition[]): string {
  const [first] = sections;
  return first ? settingsPath(first.group, first.section) : SETTINGS_ROOT;
}

export default function SettingsPage(): ReactElement {
  const location = useLocation();
  const [group, section, ...rest] = (useParams()['*'] ?? '').split('/').filter(Boolean);
  const compact = useMediaQuery(COMPACT_QUERY);
  const { access, loading, resolvingWorkspaceRole } = useSettingsAccess();
  const { currentOrganization, currentWorkspace } = useWorkspace();
  const loadingState = <div className="loading-state">Loading settings...</div>;
  const bare = group === undefined;

  let title = SETTINGS_TITLE;
  let body: ReactNode;

  if (loading) {
    body = loadingState;
  } else {
    const sections = visibleSections(access);
    const nav = <SettingsNav sections={sections} />;
    const definition = rest.length === 0 ? findSection(group, section) : undefined;
    const reachable = definition !== undefined && sections.includes(definition);

    if (!resolvingWorkspaceRole) {
      const fallback = relocate(firstPath(sections), location.search, location.hash);
      if (bare) {
        const legacy = legacyWorkspacePath(location.search, location.hash);
        if (legacy !== null) {
          return <Navigate to={legacy} replace />;
        }
        if (!compact) {
          return <Navigate to={fallback} replace />;
        }
      } else if (!reachable) {
        return <Navigate to={fallback} replace />;
      }
    }

    const content =
      reachable && !resolvingWorkspaceRole ? (
        <SettingsContent
          key={`${currentOrganization?.id}:${currentWorkspace?.id}:${definition.group}/${definition.section}`}
          group={definition.group}
          section={definition.section}
          access={access}
        />
      ) : (
        loadingState
      );
    const label = reachable ? definition.label : undefined;
    const area = (
      <section className="settings-content" aria-label={label}>
        {content}
      </section>
    );

    if (bare && compact) {
      body = nav;
    } else if (compact) {
      title = label ?? SETTINGS_TITLE;
      body = (
        <>
          <Link to={SETTINGS_ROOT} className="settings-back" aria-label="Back to settings">
            Settings
          </Link>
          {area}
        </>
      );
    } else {
      body = (
        <div className="settings-layout">
          <aside className="settings-sidebar">{nav}</aside>
          {area}
        </div>
      );
    }
  }

  return (
    <div className="page page--workspace settings-page settings-shell">
      <PageBar title={title} />
      <div className="page-body">{body}</div>
    </div>
  );
}
