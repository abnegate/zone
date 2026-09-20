import { Tabs } from '@zone/ui';
import type { ReactNode } from 'react';
import PageBar from '../../../shared/components/PageBar/PageBar';
import '../workspace/pages/WorkspaceSettingsPage.css';

interface SettingsPageProps {
  title: string;
  tabs?: ReactNode;
  value?: string;
  onValueChange?: (value: string) => void;
  children: ReactNode;
}

export function SettingsPage({ title, tabs, value, onValueChange, children }: SettingsPageProps) {
  return (
    <Tabs
      value={value}
      onValueChange={onValueChange}
      className="page page--workspace settings-page"
    >
      <PageBar title={title}>{tabs}</PageBar>
      <div className="page-body">
        <div className="page-container settings-body">{children}</div>
      </div>
    </Tabs>
  );
}
