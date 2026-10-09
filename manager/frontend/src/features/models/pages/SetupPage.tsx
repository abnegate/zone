import { Button } from '@zone/ui';
import { useCallback, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import ZoneLogo from '../../../shared/components/ZoneLogo';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { useAuth } from '../../auth';
import { HostFoldersPanel } from '../../settings/workspace/components';
import FeaturesPanel from '../components/FeaturesPanel';
import PullJobs from '../components/PullJobs';
import { usePull } from '../hooks/usePull';
import { markSetupComplete } from '../setupComplete';
import './SetupPage.css';

type Step = 'features' | 'folders';

export default function SetupPage() {
  const { user } = useAuth();
  const { currentOrganization, currentWorkspace } = useWorkspace();
  const pull = usePull();
  const navigate = useNavigate();
  const [done, setDone] = useState(false);
  const [step, setStep] = useState<Step>('features');

  const finish = useCallback(() => {
    if (user?.id) markSetupComplete(user.id);
    navigate('/', { replace: true });
  }, [navigate, user?.id]);

  const onInstalled = useCallback(() => {
    setDone(true);
  }, []);

  const toFolders = useCallback(() => {
    setStep('folders');
  }, []);

  return (
    <main className="setup-page">
      <div className="setup-panel">
        <header className="setup-header">
          <ZoneLogo size="md" />
          <h1>Set up Zone</h1>
          {step === 'features' ? (
            <p>Pick the features this install should run. Chat stays on.</p>
          ) : (
            <p>Choose host folders chat tools can use on this machine.</p>
          )}
          <p className="setup-steps" aria-current={step}>
            <span className={step === 'features' ? 'setup-step setup-step--current' : 'setup-step'}>
              1 Features
            </span>
            <span aria-hidden="true"> · </span>
            <span className={step === 'folders' ? 'setup-step setup-step--current' : 'setup-step'}>
              2 Folders
            </span>
          </p>
        </header>

        {step === 'features' ? (
          <>
            <FeaturesPanel variant="setup" pull={pull} onInstalled={onInstalled} />

            {pull.jobs.length > 0 && (
              <section className="setup-downloads" aria-label="Downloads">
                <h2>Downloads</h2>
                <PullJobs jobs={pull.jobs} onCancel={pull.cancel} onDismiss={pull.dismiss} />
              </section>
            )}

            {done ? (
              <div className="setup-continue">
                <Button variant="primary" onClick={toFolders}>
                  Continue
                </Button>
              </div>
            ) : (
              <div className="setup-footer">
                <Button variant="link" onClick={toFolders}>
                  Skip for now
                </Button>
              </div>
            )}
          </>
        ) : (
          <HostFoldersPanel
            workspaceId={currentWorkspace?.id ?? null}
            orgId={currentOrganization?.id ?? null}
            variant="setup"
            onSaved={finish}
            onSkip={finish}
          />
        )}
      </div>
    </main>
  );
}
