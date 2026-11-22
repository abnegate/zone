import { Button } from '@zone/ui';
import { useId, useState } from 'react';
import { LapsedNotice } from '../../components/LapsedNotice';
import { UpgradeNotice } from '../../components/UpgradeNotice';
import { community, lapsed } from '../../policy/license';
import { useFleetPolicy, useRunners } from '../hooks';
import {
  LAPSED_NOTICE,
  NOT_ENFORCED,
  READ_ONLY,
  RUNNERS_DESCRIPTION,
  RUNNERS_TITLE,
  RunnerAction,
  type StateAction,
  UPGRADE_NOTICE,
} from '../messages';
import type { FleetPolicyResponse, RunnerResponse, RunnersResponse } from '../types';
import { MinimumVersionField } from './MinimumVersionField';
import { RemoveRunnerDialog } from './RemoveRunnerDialog';
import { RunnerStateDialog } from './RunnerStateDialog';
import { RunnersTable } from './RunnersTable';
import './RunnersSection.css';

interface Chosen {
  runner: RunnerResponse;
  action: RunnerAction;
}

interface FleetProps {
  policy: FleetPolicyResponse;
  listed: RunnersResponse;
}

function isStateAction(action: RunnerAction): action is StateAction {
  return action !== RunnerAction.Remove;
}

function Fleet({ policy, listed }: FleetProps) {
  const [chosen, setChosen] = useState<Chosen | null>(null);
  const close = () => setChosen(null);

  return (
    <>
      {community(policy) && <UpgradeNotice message={UPGRADE_NOTICE} className="runners-notice" />}
      {lapsed(policy) && <LapsedNotice message={LAPSED_NOTICE} className="runners-lapsed" />}
      {!listed.enforced && (
        <p className="runners-hint" role="note">
          {NOT_ENFORCED}
        </p>
      )}
      {policy.entitled && !policy.editable && <p className="runners-hint">{READ_ONLY}</p>}
      <MinimumVersionField key={policy.minimum_version ?? ''} policy={policy} />
      <RunnersTable
        runners={listed.runners}
        editable={listed.editable}
        now={new Date()}
        onAction={(runner, action) => setChosen({ runner, action })}
      />
      {chosen &&
        (isStateAction(chosen.action) ? (
          <RunnerStateDialog
            key={`${chosen.runner.id}/${chosen.action}`}
            runner={chosen.runner}
            action={chosen.action}
            onClose={close}
          />
        ) : (
          <RemoveRunnerDialog key={chosen.runner.id} runner={chosen.runner} onClose={close} />
        ))}
    </>
  );
}

export function RunnersSection() {
  const headingId = useId();
  const fleet = useFleetPolicy();
  const runners = useRunners();
  const error = fleet.error ?? runners.error;

  if (fleet.loading || runners.loading) {
    return <div className="loading-state">Loading the runners...</div>;
  }

  const retry = () => {
    void fleet.refresh();
    void runners.refresh();
  };

  return (
    <section className="settings-section runners-section" aria-labelledby={headingId}>
      <div className="section-row">
        <div className="section-row-copy">
          <h2 id={headingId} className="section-title">
            {RUNNERS_TITLE}
          </h2>
          <p className="section-description">{RUNNERS_DESCRIPTION}</p>
        </div>
      </div>
      {error && (
        <div className="alert alert-error alert-inline" role="alert">
          <span>{error}</span>
          <Button type="button" variant="ghost" size="sm" onClick={retry}>
            Retry
          </Button>
        </div>
      )}
      {fleet.policy && runners.runners && <Fleet policy={fleet.policy} listed={runners.runners} />}
    </section>
  );
}
