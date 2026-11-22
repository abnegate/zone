import { describe, expect, it } from 'bun:test';
import { Tone } from '../status';
import { FleetRefusal } from './FleetRefusal';
import {
  actionsFor,
  actionTitle,
  consequence,
  failureText,
  matchingText,
  noMatchWarning,
  RunnerAction,
  refusalMessage,
  STATE_TARGETS,
  stateLabel,
  stateTone,
  workReason,
} from './messages';
import {
  FLEET_ADMIN_CODES,
  FLEET_WORK_CODES,
  FleetCode,
  RUNNER_STATES,
  RunnerState,
} from './schemas';

describe('fleet messages', () => {
  it('explains every reason a runner takes no work', () => {
    for (const code of FLEET_WORK_CODES) {
      expect(workReason(code), code).not.toBe('');
    }
    expect(workReason(FleetCode.RunnerRevoked)).toBe('Takes no work');
    expect(workReason(FleetCode.RunnerCordoned)).toBe('Chats only');
  });

  it('explains every refused change', () => {
    for (const code of FLEET_ADMIN_CODES) {
      expect(refusalMessage(code), code).not.toBe('');
    }
  });

  it('labels and colours every state', () => {
    expect(RUNNER_STATES.map(stateLabel)).toEqual(['Active', 'Cordoned', 'Revoked']);
    expect(RUNNER_STATES.map(stateTone)).toEqual([Tone.Success, Tone.Warning, Tone.Error]);
  });

  it('offers only the moves a state allows', () => {
    expect(actionsFor(RunnerState.Active)).toEqual([RunnerAction.Cordon, RunnerAction.Revoke]);
    expect(actionsFor(RunnerState.Cordoned)).toEqual([RunnerAction.Activate, RunnerAction.Revoke]);
    expect(actionsFor(RunnerState.Revoked)).toEqual([RunnerAction.Activate]);
    expect(STATE_TARGETS).toEqual({
      [RunnerAction.Cordon]: RunnerState.Cordoned,
      [RunnerAction.Revoke]: RunnerState.Revoked,
      [RunnerAction.Activate]: RunnerState.Active,
    });
  });

  it('says a revoke stops live task runs and ends chats', () => {
    const text = consequence(RunnerAction.Revoke, 'zone-1');

    expect(actionTitle(RunnerAction.Revoke, 'zone-1')).toBe('Revoke zone-1?');
    expect(text).toContain('live task runs stop');
    expect(text).toContain('chats end');
  });

  it('says a cordoned runner keeps serving chats', () => {
    expect(consequence(RunnerAction.Cordon, 'zone-1')).toContain('keeps serving chats');
  });

  it('shows the server wording for a version or label it refused, and the console wording otherwise', () => {
    const version = new FleetRefusal(
      { error: '"1.4" is not a valid Zone version.', code: FleetCode.VersionInvalid },
      400
    );
    const license = new FleetRefusal(
      { error: 'Requires Zone Enterprise', code: FleetCode.LicenseRequired },
      403
    );

    expect(failureText(version, version.message)).toBe('"1.4" is not a valid Zone version.');
    expect(failureText(license, license.message)).toBe(refusalMessage(FleetCode.LicenseRequired));
    expect(failureText(null, 'Network down')).toBe('Network down');
    expect(failureText(null, null)).toBeNull();
  });

  it('counts matching runners and warns when none matches', () => {
    expect(matchingText(1)).toBe('1 runner can take this work now.');
    expect(matchingText(3)).toBe('3 runners can take this work now.');
    expect(noMatchWarning(['gpu', 'region=eu'])).toBe(
      'No active runner on the minimum version carries gpu, region=eu, so unattended work here waits until one does.'
    );
    expect(noMatchWarning([])).toContain('No active runner on the minimum version is connected');
  });
});
