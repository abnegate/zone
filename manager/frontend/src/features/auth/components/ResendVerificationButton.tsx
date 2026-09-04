import type { ButtonProps } from '@zone/ui';
import { Button } from '@zone/ui';
import { useState } from 'react';
import { client } from '../../../api/client';
import './ResendVerificationButton.css';

interface ResendVerificationButtonProps {
  email: string;
  onSuccess?: () => void;
  variant?: ButtonProps['variant'];
  className?: string;
}

export default function ResendVerificationButton({
  email,
  onSuccess,
  variant = 'secondary',
  className,
}: ResendVerificationButtonProps) {
  const [loading, setLoading] = useState(false);
  const [success, setSuccess] = useState(false);
  const [error, setError] = useState('');

  const handleResend = async () => {
    setLoading(true);
    setError('');

    try {
      await client.resendVerification(email);
      setSuccess(true);
      if (onSuccess) {
        onSuccess();
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to send');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className={className}>
      <Button variant={variant} onClick={handleResend} loading={loading} disabled={success}>
        {success ? 'Sent! Check your email' : loading ? 'Sending...' : 'Resend Verification'}
      </Button>
      {error && <div className="resend-verification-error">{error}</div>}
    </div>
  );
}
