import { Permission } from '../roles/schemas';
import { Tone } from '../status';
import type { FleetRefusal } from './FleetRefusal';
import { FleetCode, RunnerState } from './schemas';
import type { FleetAdminCode, FleetWorkCode, RunnerState as RunnerStateValue } from './types';

export const RUNNERS_TITLE = 'Runners';

export const RUNNERS_DESCRIPTION =
  'Every Zone server process is a runner. Cordon or revoke runners, and set the lowest version that may take work.';

export const UPGRADE_NOTICE =
  'Runner fleet policy is part of Zone Enterprise. Add a license to cordon and revoke runners, set a minimum version, and place unattended work on labelled runners.';

export const LAPSED_NOTICE =
  'Your Zone license has lapsed. Fleet policy is still enforced; changing it needs an active or grace license.';

export const NOT_ENFORCED =
  'Nothing here is enforced, so every runner takes work whatever its state, version or labels. Saved states, the minimum version and label requirements are kept for when an Enterprise license is added.';

export const UNVERIFIED_NOTICE =
  'The stored Zone license key no longer verifies. Fleet policy is still enforced, and changing it is blocked until a server admin replaces or removes the key.';

export const LOADING_RUNNERS = 'Loading the runners...';

export const RETRY_LABEL = 'Retry';

export const UPDATED_LABEL = 'Last changed';

export const MINIMUM_TITLE = 'Minimum version';

export const MINIMUM_DESCRIPTION =
  'Runners below this version take no task runs, unattended turns or chats. Their task runs move to a current runner, or fail with a message that names the minimum.';

export const MINIMUM_LABEL = 'Lowest Zone version that may take work';

export const MINIMUM_PLACEHOLDER = '1.4.0';

export const NO_MINIMUM = 'No minimum. Runners on any version take work.';

export const SAVE_MINIMUM = 'Save minimum';

export const CLEAR_MINIMUM = 'Clear minimum';

export const RUNNERS_TABLE_TITLE = 'Fleet';

export const NO_RUNNERS = 'No runner has registered yet.';

export const RUNNER_COLUMN = 'Runner';

export const LABELS_COLUMN = 'Labels';

export const STATE_COLUMN = 'State';

export const SEEN_COLUMN = 'Seen';

export const RUNS_COLUMN = 'Runs';

export const ACTIONS_COLUMN = 'Actions';

export const CURRENT_BADGE = 'This server';

export const OUTDATED_BADGE = 'Outdated';

export const NO_LABELS = 'No labels';

export const SERVING = 'Serving';

export const OFFLINE = 'Offline';

export const CONNECTED = 'Connected';

export const CANCEL_LABEL = 'Cancel';

export const NO_RUNNER_LEFT_TITLE = 'Leave no runner able to take work?';

export const NO_RUNNER_LEFT_CONFIRM = 'Do it anyway';

export const NO_RUNNER_LEFT_DETAIL =
  'Unattended work waits, and task runs fail, until a runner can take them again.';

export const RunnerAction = {
  Cordon: 'cordon',
  Revoke: 'revoke',
  Activate: 'activate',
  Remove: 'remove',
} as const;

export type RunnerAction = (typeof RunnerAction)[keyof typeof RunnerAction];

export const ACTION_LABELS: Readonly<Record<RunnerAction, string>> = {
  [RunnerAction.Cordon]: 'Cordon',
  [RunnerAction.Revoke]: 'Revoke',
  [RunnerAction.Activate]: 'Activate',
  [RunnerAction.Remove]: 'Remove',
};

const CONFIRM_LABELS: Readonly<Record<RunnerAction, string>> = {
  [RunnerAction.Cordon]: 'Cordon runner',
  [RunnerAction.Revoke]: 'Revoke runner',
  [RunnerAction.Activate]: 'Activate runner',
  [RunnerAction.Remove]: 'Remove runner',
};

const CONSEQUENCES: Readonly<Record<RunnerAction, (name: string) => string>> = {
  [RunnerAction.Cordon]: (name) =>
    `${name} finishes the work it has and keeps serving chats, but takes no new task runs, reminder turns or auto-project drives.`,
  [RunnerAction.Revoke]: (name) =>
    `${name} takes no more work. Its live task runs stop and its chats end; people in them reload to reach another server. The stopped runs' tasks stay paused until someone resumes them.`,
  [RunnerAction.Activate]: (name) =>
    `${name} takes work again, as long as it runs the minimum version or later and carries the labels the work needs.`,
  [RunnerAction.Remove]: (name) =>
    `Zone forgets ${name} and its state. If its server starts again, it registers as a new active runner.`,
};

export const CURRENT_WARNING =
  'This is the server answering this page, so your own chats on it end too. Reload the page to reach another server.';

export type StateAction = Exclude<RunnerAction, typeof RunnerAction.Remove>;

export const STATE_TARGETS: Readonly<Record<StateAction, RunnerStateValue>> = {
  [RunnerAction.Cordon]: RunnerState.Cordoned,
  [RunnerAction.Revoke]: RunnerState.Revoked,
  [RunnerAction.Activate]: RunnerState.Active,
};

const STATE_LABELS: Readonly<Record<RunnerStateValue, string>> = {
  [RunnerState.Active]: 'Active',
  [RunnerState.Cordoned]: 'Cordoned',
  [RunnerState.Revoked]: 'Revoked',
};

const STATE_TONES: Readonly<Record<RunnerStateValue, Tone>> = {
  [RunnerState.Active]: Tone.Success,
  [RunnerState.Cordoned]: Tone.Warning,
  [RunnerState.Revoked]: Tone.Error,
};

const STATE_ACTIONS: Readonly<Record<RunnerStateValue, readonly StateAction[]>> = {
  [RunnerState.Active]: [RunnerAction.Cordon, RunnerAction.Revoke],
  [RunnerState.Cordoned]: [RunnerAction.Activate, RunnerAction.Revoke],
  [RunnerState.Revoked]: [RunnerAction.Activate],
};

const WORK_REASONS: Readonly<Record<FleetWorkCode, string>> = {
  [FleetCode.RunnerUnregistered]: 'Not registered',
  [FleetCode.RunnerCordoned]: 'Chats only',
  [FleetCode.RunnerRevoked]: 'Takes no work',
  [FleetCode.RunnerOutdated]: 'Below the minimum',
  [FleetCode.LabelsMissing]: 'Lacks required labels',
  [FleetCode.NoRunner]: 'No runner can take it',
};

const REFUSALS: Readonly<Record<FleetAdminCode, string>> = {
  [FleetCode.LicenseRequired]:
    'Changing the fleet policy needs a Zone Enterprise license that is active or in its grace period.',
  [FleetCode.BodyInvalid]: "Zone couldn't read this change. Reload the page and try again.",
  [FleetCode.VersionInvalid]:
    'Enter a version as three numbers, such as 1.4.0, with no v and no pre-release.',
  [FleetCode.LabelInvalid]:
    'Use 1 to 63 lowercase letters, digits or . _ - = /, starting with a letter or digit, such as gpu or region=eu.',
  [FleetCode.StateInvalid]: "Zone couldn't read this runner state. Reload the page and try again.",
  [FleetCode.RunnerOnline]:
    'This runner is connected, so it would only register again. Stop its server first, or cordon or revoke it instead.',
  [FleetCode.RunnerBusy]:
    'This runner still holds live work. Remove it once that work ends, or revoke it to end the work now.',
  [FleetCode.NoRunnerLeft]:
    'This would leave no runner able to take work. Confirm to do it anyway.',
};

export function stateLabel(state: RunnerStateValue): string {
  return STATE_LABELS[state];
}

export function stateTone(state: RunnerStateValue): Tone {
  return STATE_TONES[state];
}

export function actionsFor(state: RunnerStateValue): readonly StateAction[] {
  return STATE_ACTIONS[state];
}

export function confirmLabel(action: RunnerAction): string {
  return CONFIRM_LABELS[action];
}

export function actionTitle(action: RunnerAction, name: string): string {
  return `${ACTION_LABELS[action]} ${name}?`;
}

export function consequence(action: RunnerAction, name: string): string {
  return CONSEQUENCES[action](name);
}

export function workReason(code: FleetWorkCode): string {
  return WORK_REASONS[code];
}

export function refusalMessage(code: FleetAdminCode): string {
  return REFUSALS[code];
}

const NAMES_ENTRY: ReadonlySet<FleetAdminCode> = new Set([
  FleetCode.VersionInvalid,
  FleetCode.LabelInvalid,
]);

export function failureText(refusal: FleetRefusal | null, error: string | null): string | null {
  if (refusal === null) {
    return error;
  }
  const named = NAMES_ENTRY.has(refusal.code) ? refusal.message.trim() : '';
  return named || refusalMessage(refusal.code);
}

export function lastSeen(relative: string): string {
  return `Last seen ${relative}`;
}

export const PLACEMENT_TITLE = 'Runner placement';

export const PLACEMENT_DESCRIPTION =
  'Task runs, reminder turns and auto-project drives run only on runners that carry every required label.';

export const CHATS_IGNORE_LABELS =
  'Chats ignore labels and run on whichever server people reach, so labels place work but do not keep data in one place.';

export const PLACEMENT_LAPSED =
  'Your Zone license has lapsed. Label requirements are still enforced; changing them needs an active or grace license.';

export const PLACEMENT_UNVERIFIED =
  'The stored Zone license key no longer verifies. Label requirements are still enforced; changing them is blocked until a server admin replaces or removes the key.';

export const LOADING_PLACEMENT = 'Loading runner placement...';

export const ORGANIZATION_PLACEMENT_READ_ONLY = `Changing the runner labels needs the ${Permission.PoliciesUpdate} permission in this organization.`;

export const WORKSPACE_PLACEMENT_READ_ONLY = `Changing this workspace's runner labels needs the ${Permission.WorkspacePoliciesUpdate} permission in this workspace.`;

export const REQUIRED_LABEL = 'Required labels';

export const ORGANIZATION_REQUIRED_LABEL = 'Required by the organization';

export const WORKSPACE_REQUIRED_LABEL = 'Required by this workspace';

export const EFFECTIVE_LABEL = 'In effect';

export const NOTHING_REQUIRED = 'None. Any runner can take this work.';

export const INHERITED_NOTE = 'Set on the organization. A workspace can only add labels.';

export const ADD_LABEL = 'Add';

export const ENTRY_LABEL = 'Add a required label';

export const ENTRY_PLACEHOLDER = 'region=eu, agents=claude';

export const AVAILABLE_LABEL = 'Labels runners carry';

export const NO_AVAILABLE = 'No connected runner carries a label yet.';

export const SAVE_LABELS = 'Save labels';

export const DISCARD_LABELS = 'Discard changes';

export function runnerCount(runners: number): string {
  return runners === 1 ? '1 runner' : `${runners} runners`;
}

export function requireLabel(label: string): string {
  return `Require ${label}`;
}

export function removeLabel(label: string): string {
  return `Remove ${label}`;
}

export function matchingText(matching: number): string {
  return `${runnerCount(matching)} can take this work now.`;
}

export function noMatchWarning(effective: readonly string[]): string {
  return effective.length === 0
    ? 'No active runner on the minimum version is connected, so task runs fail at once, and reminder turns and auto-project drives wait until one is.'
    : `No active runner on the minimum version carries ${effective.join(', ')}, so task runs here fail at once, and reminder turns and auto-project drives wait until one does.`;
}

export function invalidLabels(labels: readonly string[]): string {
  return `${labels.join(', ')} ${labels.length === 1 ? 'is not a valid label' : 'are not valid labels'}. ${REFUSALS[FleetCode.LabelInvalid]}`;
}

export function inheritedAlready(labels: readonly string[]): string {
  return `The organization already requires ${labels.join(', ')}.`;
}

export function pendingLabels(labels: readonly string[]): string {
  return `Saving adds ${labels.join(', ')}.`;
}
