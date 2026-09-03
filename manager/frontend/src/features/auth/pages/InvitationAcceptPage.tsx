import { Button } from '@zone/ui';
import { useCallback, useEffect, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { client } from '../../../api/client';
import { useAuth } from '../hooks';
import type { InvitationDetails } from '../types';
import './InvitationAcceptPage.css';

export default function InvitationAcceptPage() {
  const [searchParams] = useSearchParams();
  const navigate = useNavigate();
  const { isAuthenticated } = useAuth();

  const token = searchParams.get('token');
  const [details, setDetails] = useState<InvitationDetails | null>(null);
  const [loading, setLoading] = useState(true);
  const [accepting, setAccepting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadInvitation = useCallback(async () => {
    if (!token) {
      setError('Invalid invitation link');
      setLoading(false);
      return;
    }

    setLoading(true);
    setError(null);
    try {
      const invitationDetails = await client.getInvitationByToken(token);
      setDetails(invitationDetails);

      // Check if expired
      if (new Date(invitationDetails.expires_at) < new Date()) {
        setError('This invitation has expired');
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load invitation');
    } finally {
      setLoading(false);
    }
  }, [token]);

  useEffect(() => {
    loadInvitation();
  }, [loadInvitation]);

  const handleAccept = async () => {
    if (!token) return;

    setAccepting(true);
    setError(null);

    try {
      await client.acceptInvitation(token);
      // Redirect to org-settings or home after successful acceptance
      navigate('/org-settings');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to accept invitation');
    } finally {
      setAccepting(false);
    }
  };

  const handleLoginRedirect = () => {
    navigate(
      `/login?redirect=${encodeURIComponent(window.location.pathname + window.location.search)}`
    );
  };

  const handleRegisterRedirect = () => {
    navigate(
      `/register?redirect=${encodeURIComponent(window.location.pathname + window.location.search)}`
    );
  };

  const formatDate = (dateString: string) => {
    return new Date(dateString).toLocaleDateString('en-US', {
      year: 'numeric',
      month: 'long',
      day: 'numeric',
    });
  };

  if (loading) {
    return (
      <div className="invitation-accept-page">
        <div className="invitation-card">
          <div className="loading-state">Loading invitation...</div>
        </div>
      </div>
    );
  }

  if (error && !details) {
    return (
      <div className="invitation-accept-page">
        <div className="invitation-card">
          <div className="error-state">
            <h1>Invalid Invitation</h1>
            <p>{error}</p>
            <Button onClick={() => navigate('/')} variant="primary">
              Go to Home
            </Button>
          </div>
        </div>
      </div>
    );
  }

  if (!details) {
    return (
      <div className="invitation-accept-page">
        <div className="invitation-card">
          <div className="error-state">
            <h1>Invitation Not Found</h1>
            <p>This invitation link is invalid or has been revoked.</p>
            <Button onClick={() => navigate('/')} variant="primary">
              Go to Home
            </Button>
          </div>
        </div>
      </div>
    );
  }

  const isExpired = new Date(details.expires_at) < new Date();

  return (
    <div className="invitation-accept-page">
      <div className="invitation-card">
        <div className="invitation-header">
          <h1>You've Been Invited!</h1>
          <p className="subtitle">
            {details.invited_by_email} has invited you to join{' '}
            <strong>{details.organization_name}</strong>
          </p>
        </div>

        <div className="invitation-details">
          <div className="detail-row">
            <span className="label">Organization:</span>
            <span className="value">{details.organization_name}</span>
          </div>

          <div className="detail-row">
            <span className="label">Organization Role:</span>
            <span className={`value role-badge role-${details.org_role}`}>{details.org_role}</span>
          </div>

          {details.workspace_name && (
            <>
              <div className="detail-row">
                <span className="label">Workspace:</span>
                <span className="value">{details.workspace_name}</span>
              </div>

              {details.workspace_role && (
                <div className="detail-row">
                  <span className="label">Workspace Role:</span>
                  <span className={`value role-badge role-${details.workspace_role}`}>
                    {details.workspace_role}
                  </span>
                </div>
              )}
            </>
          )}

          <div className="detail-row">
            <span className="label">Invited By:</span>
            <span className="value">{details.invited_by_email}</span>
          </div>

          <div className="detail-row">
            <span className="label">Expires:</span>
            <span className={`value ${isExpired ? 'text-danger' : ''}`}>
              {formatDate(details.expires_at)}
              {isExpired && ' (Expired)'}
            </span>
          </div>
        </div>

        {error && <div className="alert alert-error">{error}</div>}

        <div className="invitation-actions">
          {!isAuthenticated ? (
            <>
              <p className="auth-notice">You need to be signed in to accept this invitation</p>
              <div className="auth-buttons">
                <Button onClick={handleLoginRedirect} variant="primary" size="lg">
                  Sign In
                </Button>
                <Button onClick={handleRegisterRedirect} variant="secondary" size="lg">
                  Create Account
                </Button>
              </div>
            </>
          ) : isExpired ? (
            <p className="text-danger">
              This invitation has expired and can no longer be accepted.
            </p>
          ) : (
            <Button
              onClick={handleAccept}
              variant="primary"
              size="lg"
              loading={accepting}
              disabled={accepting}
            >
              Accept Invitation
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}
