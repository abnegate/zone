import type { ReactNode } from 'react';
import { useEffect, useState } from 'react';
import { Navigate } from 'react-router-dom';
import { modelsApi } from '../../../api/models';
import { PERMISSIONS } from '../../../shared/types/permissions';
import { useAuth } from '../../auth';
import { isSetupComplete, markSetupComplete } from '../setupComplete';

function Loading() {
  return (
    <div className="loading-container">
      <span className="spinner" />
      <span>Loading...</span>
    </div>
  );
}

export default function SetupGate({ children }: { children: ReactNode }) {
  const { user, hasPermission } = useAuth();
  const userId = user?.id;
  const canReadModels = hasPermission(PERMISSIONS.MODELS.READ);
  const skip = !userId || !canReadModels || isSetupComplete(userId);
  const [needed, setNeeded] = useState<boolean | null>(skip ? false : null);

  useEffect(() => {
    if (skip) {
      setNeeded(false);
      return;
    }
    let cancelled = false;
    setNeeded(null);
    void modelsApi
      .getSetup()
      .then((plan) => {
        if (cancelled) return;
        if (plan.pulls.length === 0) {
          if (userId) markSetupComplete(userId);
          setNeeded(false);
          return;
        }
        setNeeded(true);
      })
      .catch(() => {
        if (!cancelled) setNeeded(false);
      });
    return () => {
      cancelled = true;
    };
  }, [skip, userId]);

  if (needed === null) {
    return <Loading />;
  }
  if (needed) {
    return <Navigate to="/setup" replace />;
  }
  return <>{children}</>;
}
