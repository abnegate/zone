import { Button } from '@zone/ui';
import { useEffect, useState } from 'react';
import { client } from '../../../api/client';
import './VerificationPendingBanner.css';

interface VerificationPendingBannerProps {
  email: string;
  onDismiss?: () => void;
}

export default function VerificationPendingBanner({
  email,
  onDismiss,
}: VerificationPendingBannerProps) {
  const [loading, setLoading] = useState(false);
  const [success, setSuccess] = useState(false);
  const [error, setError] = useState('');
  const [lastResendTime, setLastResendTime] = useState<number | null>(null);
  const [cooldownRemaining, setCooldownRemaining] = useState(0);

  const COOLDOWN_SECONDS = 60;

  useEffect(() => {
    if (success) {
      const timer = setTimeout(() => {
        setSuccess(false);
      }, 5000);
      return () => clearTimeout(timer);
    }
  }, [success]);

  // Cooldown timer
  useEffect(() => {
    if (lastResendTime) {
      const interval = setInterval(() => {
        const elapsed = Math.floor((Date.now() - lastResendTime) / 1000);
        const remaining = Math.max(0, COOLDOWN_SECONDS - elapsed);
        setCooldownRemaining(remaining);
      }, 1000);

      return () => clearInterval(interval);
    }
  }, [lastResendTime]);

  const handleResend = async () => {
    // Check cooldown
    if (lastResendTime) {
      const elapsed = Math.floor((Date.now() - lastResendTime) / 1000);
      if (elapsed < COOLDOWN_SECONDS) {
        setError(`Please wait ${COOLDOWN_SECONDS - elapsed} seconds before resending`);
        return;
      }
    }

    setLoading(true);
    setError('');

    try {
      await client.resendVerification(email);
      setSuccess(true);
      setLastResendTime(Date.now());
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to send verification email');
    } finally {
      setLoading(false);
    }
  };

  const handleDismiss = () => {
    if (onDismiss) {
      onDismiss();
    }
  };

  return (
    <div className="verification-banner verification-banner--warning">
      <div className="verification-banner__content">
        <div className="verification-banner__icon">
          <svg
            width="20"
            height="20"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
            <line x1="12" y1="9" x2="12" y2="13" />
            <line x1="12" y1="17" x2="12.01" y2="17" />
          </svg>
        </div>
        <div className="verification-banner__text">
          <strong>Email not verified</strong>
          <span>Please verify your email address to access all features.</span>
        </div>
        <div className="verification-banner__actions">
          {success ? (
            <div className="verification-banner__success">
              <span data-testid="success-icon">
                <svg
                  width="16"
                  height="16"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                >
                  <polyline points="20 6 9 17 4 12" />
                </svg>
              </span>
              <span>Verification email sent</span>
            </div>
          ) : (
            <Button
              variant="secondary"
              size="sm"
              onClick={handleResend}
              loading={loading}
              disabled={cooldownRemaining > 0}
            >
              {loading
                ? 'Sending...'
                : cooldownRemaining > 0
                  ? `Resend (${cooldownRemaining}s)`
                  : 'Resend Verification Email'}
            </Button>
          )}
          <button
            type="button"
            className="verification-banner__dismiss"
            onClick={handleDismiss}
            aria-label="Dismiss"
          >
            <svg
              width="16"
              height="16"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>
      </div>
      {error && <div className="verification-banner__error">{error}</div>}
    </div>
  );
}
