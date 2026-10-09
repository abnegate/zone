import { Button } from '@zone/ui';
import { useCallback, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import ZoneLogo from '../../../shared/components/ZoneLogo';
import { useAuth } from '../../auth';
import FeaturesPanel from '../components/FeaturesPanel';
import PullJobs from '../components/PullJobs';
import { usePull } from '../hooks/usePull';
import { markSetupComplete } from '../setupComplete';
import './SetupPage.css';

export default function SetupPage() {
  const { user } = useAuth();
  const pull = usePull();
  const navigate = useNavigate();
  const [done, setDone] = useState(false);

  const finish = useCallback(() => {
    if (user?.id) markSetupComplete(user.id);
    navigate('/', { replace: true });
  }, [navigate, user?.id]);

  const onInstalled = useCallback(() => {
    setDone(true);
  }, []);

  return (
    <main className="setup-page">
      <div className="setup-panel">
        <header className="setup-header">
          <ZoneLogo size="md" />
          <h1>Set up Zone</h1>
          <p>Pick the features this install should run. Chat stays on.</p>
        </header>

        <FeaturesPanel variant="setup" pull={pull} onInstalled={onInstalled} />

        {pull.jobs.length > 0 && (
          <section className="setup-downloads" aria-label="Downloads">
            <h2>Downloads</h2>
            <PullJobs jobs={pull.jobs} onCancel={pull.cancel} onDismiss={pull.dismiss} />
          </section>
        )}

        {done ? (
          <div className="setup-continue">
            <Button variant="primary" onClick={finish}>
              Continue to Zone
            </Button>
          </div>
        ) : (
          <div className="setup-footer">
            <Button variant="link" onClick={finish}>
              Skip for now
            </Button>
          </div>
        )}
      </div>
    </main>
  );
}
