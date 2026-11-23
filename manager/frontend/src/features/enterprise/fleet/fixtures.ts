import { Edition, LicenseStatus } from '../schemas';
import { FleetCode, RunnerState } from './schemas';
import type {
  FleetPolicyResponse,
  OrganizationRunnerLabelsResponse,
  RunnerResponse,
  RunnersResponse,
  WorkspaceRunnerLabelsResponse,
} from './types';

export const ORGANIZATION_ID = '9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c6d';

export const WORKSPACE_ID = '1f2e3d4c-5b6a-4978-8a6b-5c4d3e2f1a0b';

export const NOW = '2026-11-23T12:00:00Z';

export const UPDATED_AT = '2026-11-20T09:30:00Z';

export const ACTIVE_POLICY: FleetPolicyResponse = {
  entitled: true,
  enforced: true,
  edition: Edition.Enterprise,
  status: LicenseStatus.Active,
  editable: true,
  minimum_version: '1.4.0',
  updated_at: UPDATED_AT,
};

export const OPEN_POLICY: FleetPolicyResponse = {
  ...ACTIVE_POLICY,
  minimum_version: null,
  updated_at: null,
};

export const LAPSED_POLICY: FleetPolicyResponse = {
  ...ACTIVE_POLICY,
  entitled: false,
  status: LicenseStatus.Lapsed,
  editable: false,
};

export const UNVERIFIED_POLICY: FleetPolicyResponse = {
  ...ACTIVE_POLICY,
  entitled: false,
  edition: Edition.Community,
  status: LicenseStatus.Community,
  editable: false,
};

export const COMMUNITY_POLICY: FleetPolicyResponse = {
  entitled: false,
  enforced: false,
  edition: Edition.Community,
  status: LicenseStatus.Community,
  editable: false,
  minimum_version: null,
  updated_at: null,
};

export const CURRENT_RUNNER: RunnerResponse = {
  id: '0b9f6a2e-3c1d-4e5f-8a7b-6c5d4e3f2a1b',
  name: 'zone-server-7f9c',
  version: '1.4.2',
  labels: ['gpu', 'region=eu'],
  state: RunnerState.Active,
  connected: true,
  outdated: false,
  serving: true,
  code: null,
  current: true,
  runs: 3,
  started_at: '2026-11-23T08:00:00Z',
  last_seen_at: '2026-11-23T11:59:50Z',
  state_changed_at: null,
};

export const CORDONED_RUNNER: RunnerResponse = {
  ...CURRENT_RUNNER,
  id: '1c2d3e4f-5a6b-4c7d-8e9f-0a1b2c3d4e5f',
  name: 'zone-server-a12b',
  labels: ['region=eu'],
  state: RunnerState.Cordoned,
  serving: false,
  code: FleetCode.RunnerCordoned,
  current: false,
  runs: 1,
  state_changed_at: UPDATED_AT,
};

export const OUTDATED_RUNNER: RunnerResponse = {
  ...CURRENT_RUNNER,
  id: '2d3e4f5a-6b7c-4d8e-9f0a-1b2c3d4e5f6a',
  name: 'zone-server-c34d',
  version: '1.3.9',
  labels: [],
  outdated: true,
  serving: false,
  code: FleetCode.RunnerOutdated,
  current: false,
  runs: 0,
};

export const REVOKED_RUNNER: RunnerResponse = {
  ...CURRENT_RUNNER,
  id: '3e4f5a6b-7c8d-4e9f-8a1b-2c3d4e5f6a7b',
  name: 'zone-server-e56f',
  labels: ['gpu'],
  state: RunnerState.Revoked,
  connected: false,
  serving: false,
  code: FleetCode.RunnerRevoked,
  current: false,
  runs: 0,
  last_seen_at: '2026-11-23T09:00:00Z',
  state_changed_at: UPDATED_AT,
};

export const OFFLINE_RUNNER: RunnerResponse = {
  ...CURRENT_RUNNER,
  id: '4f5a6b7c-8d9e-4f0a-9b2c-3d4e5f6a7b8c',
  name: 'zone-server-0a1b',
  labels: [],
  connected: false,
  serving: false,
  code: null,
  current: false,
  runs: 0,
  last_seen_at: '2026-11-22T12:00:00Z',
};

export const RUNNERS: RunnerResponse[] = [
  CURRENT_RUNNER,
  CORDONED_RUNNER,
  OUTDATED_RUNNER,
  REVOKED_RUNNER,
  OFFLINE_RUNNER,
];

export const ACTIVE_RUNNERS: RunnersResponse = {
  editable: true,
  enforced: true,
  current: CURRENT_RUNNER.id,
  runners: RUNNERS,
};

export const COMMUNITY_RUNNERS: RunnersResponse = {
  editable: false,
  enforced: false,
  current: CURRENT_RUNNER.id,
  runners: [CURRENT_RUNNER, CORDONED_RUNNER],
};

export const LAPSED_RUNNERS: RunnersResponse = {
  ...ACTIVE_RUNNERS,
  editable: false,
};

export const UNVERIFIED_RUNNERS: RunnersResponse = LAPSED_RUNNERS;

export const AVAILABLE = [
  { label: 'gpu', runners: 1 },
  { label: 'region=eu', runners: 2 },
  { label: 'region=us', runners: 1 },
];

export const ORGANIZATION_LABELS: OrganizationRunnerLabelsResponse = {
  entitled: true,
  enforced: true,
  edition: Edition.Enterprise,
  status: LicenseStatus.Active,
  editable: true,
  labels: ['region=eu'],
  available: AVAILABLE,
  matching: 2,
  updated_at: UPDATED_AT,
};

export const WORKSPACE_LABELS: WorkspaceRunnerLabelsResponse = {
  entitled: true,
  enforced: true,
  edition: Edition.Enterprise,
  status: LicenseStatus.Active,
  editable: true,
  organization: ['region=eu'],
  workspace: ['gpu'],
  effective: ['gpu', 'region=eu'],
  available: AVAILABLE,
  matching: 1,
  updated_at: UPDATED_AT,
};

export const UNMATCHED_WORKSPACE_LABELS: WorkspaceRunnerLabelsResponse = {
  ...WORKSPACE_LABELS,
  workspace: ['region=us'],
  effective: ['region=eu', 'region=us'],
  matching: 0,
};

export const LAPSED_ORGANIZATION_LABELS: OrganizationRunnerLabelsResponse = {
  ...ORGANIZATION_LABELS,
  entitled: false,
  status: LicenseStatus.Lapsed,
  editable: false,
  available: [],
};

export const UNVERIFIED_ORGANIZATION_LABELS: OrganizationRunnerLabelsResponse = {
  ...LAPSED_ORGANIZATION_LABELS,
  edition: Edition.Community,
  status: LicenseStatus.Community,
};

export const UNVERIFIED_WORKSPACE_LABELS: WorkspaceRunnerLabelsResponse = {
  ...WORKSPACE_LABELS,
  entitled: false,
  edition: Edition.Community,
  status: LicenseStatus.Community,
  editable: false,
  available: [],
};

export const COMMUNITY_ORGANIZATION_LABELS: OrganizationRunnerLabelsResponse = {
  entitled: false,
  enforced: false,
  edition: Edition.Community,
  status: LicenseStatus.Community,
  editable: false,
  labels: [],
  available: [],
  matching: 0,
  updated_at: null,
};

export const COMMUNITY_WORKSPACE_LABELS: WorkspaceRunnerLabelsResponse = {
  entitled: false,
  enforced: false,
  edition: Edition.Community,
  status: LicenseStatus.Community,
  editable: false,
  organization: [],
  workspace: [],
  effective: [],
  available: [],
  matching: 0,
  updated_at: null,
};

export const NO_RUNNER_LEFT = {
  error: 'This would leave no runner able to take work. Confirm to do it anyway.',
  code: FleetCode.NoRunnerLeft,
  remaining: 0,
};
