import { useCallback, useEffect, useRef, useState } from 'react';
import { client } from '../../../api/client';
import { Button, Modal } from '@zone/ui';
import { useAuth } from '../hooks';
import type { Session } from '../types';
import './SessionsPage.css';

// Constants
const SUCCESS_MESSAGE_DURATION_MS = 3000;
const REVOKE_DEBOUNCE_MS = 1000;

// Parse user agent to friendly device/browser name
function parseUserAgent(userAgent: string | null): string {
  if (!userAgent) return 'Unknown Device';

  // Handle empty or invalid strings
  if (typeof userAgent !== 'string' || userAgent.trim() === '') {
    return 'Unknown Device';
  }

  const ua = userAgent.toLowerCase();

  // Mobile detection first
  const isMobile = ua.includes('mobile') || ua.includes('android') || ua.includes('iphone');

  // Browser detection (order matters - more specific first)
  let browser = 'Unknown Browser';
  if (ua.includes('edg/') || ua.includes('edge/')) browser = 'Edge';
  else if (ua.includes('opr/') || ua.includes('opera/')) browser = 'Opera';
  else if (ua.includes('firefox/')) browser = 'Firefox';
  else if (ua.includes('chrome/')) browser = 'Chrome';
  else if (ua.includes('safari/')) browser = 'Safari';

  // OS detection (order matters - more specific first)
  let os = 'Unknown OS';
  if (ua.includes('iphone')) os = 'iOS (iPhone)';
  else if (ua.includes('ipad')) os = 'iOS (iPad)';
  else if (ua.includes('android')) os = 'Android';
  else if (ua.includes('windows')) os = 'Windows';
  else if (ua.includes('mac os x') || ua.includes('macintosh')) os = 'MacOS';
  else if (ua.includes('linux')) os = 'Linux';

  const deviceType = isMobile && !ua.includes('ipad') ? 'Mobile ' : '';

  // Return "Unknown device" for completely unknown combinations
  if (browser === 'Unknown Browser' && os === 'Unknown OS') {
    return 'Unknown Device';
  }

  return `${deviceType}${browser} on ${os}`;
}

// Format relative timestamp
function formatRelativeTime(timestamp: string): string {
  const now = new Date();
  const time = new Date(timestamp);

  // Handle invalid dates
  if (Number.isNaN(time.getTime())) {
    return 'Invalid date';
  }

  const diffMs = now.getTime() - time.getTime();

  // Handle future dates
  if (diffMs < 0) {
    return 'In the future';
  }

  const diffMins = Math.floor(diffMs / 60000);
  const diffHours = Math.floor(diffMs / 3600000);
  const diffDays = Math.floor(diffMs / 86400000);

  if (diffMins < 1) return 'Just now';
  if (diffMins < 60) return `${diffMins} minute${diffMins === 1 ? '' : 's'} ago`;
  if (diffHours < 24) return `${diffHours} hour${diffHours === 1 ? '' : 's'} ago`;
  if (diffDays < 7) return `${diffDays} day${diffDays === 1 ? '' : 's'} ago`;
  return time.toLocaleDateString();
}

export default function SessionsPage() {
  const { isAuthenticated } = useAuth();

  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);

  // Modal states
  const [showRevokeModal, setShowRevokeModal] = useState(false);
  const [showRevokeAllModal, setShowRevokeAllModal] = useState(false);
  const [sessionToRevoke, setSessionToRevoke] = useState<string | null>(null);
  const [revoking, setRevoking] = useState(false);

  // Refs for cleanup and debouncing
  const abortControllerRef = useRef<AbortController | null>(null);
  const successTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const lastRevokeTimeRef = useRef<number>(0);

  const loadSessions = useCallback(async () => {
    if (!isAuthenticated) return;

    // Cancel any pending request
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
    }

    // Create new AbortController for this request
    const abortController = new AbortController();
    abortControllerRef.current = abortController;

    setLoading(true);
    setError(null);
    try {
      const response = await client.getSessions();

      // Check if request was aborted
      if (abortController.signal.aborted) {
        return;
      }

      setSessions(response.sessions);
    } catch (err) {
      // Ignore abort errors
      if (abortController.signal.aborted) {
        return;
      }
      setError(err instanceof Error ? err.message : 'Failed to load sessions');
    } finally {
      if (!abortController.signal.aborted) {
        setLoading(false);
      }
    }
  }, [isAuthenticated]);

  useEffect(() => {
    loadSessions();

    // Cleanup on unmount
    return () => {
      if (abortControllerRef.current) {
        abortControllerRef.current.abort();
      }
      if (successTimeoutRef.current) {
        clearTimeout(successTimeoutRef.current);
      }
    };
  }, [loadSessions]);

  const handleRevokeClick = (sessionId: string) => {
    setSessionToRevoke(sessionId);
    setShowRevokeModal(true);
  };

  const setSuccessWithTimeout = (message: string) => {
    // Clear any existing timeout
    if (successTimeoutRef.current) {
      clearTimeout(successTimeoutRef.current);
    }

    setSuccess(message);
    successTimeoutRef.current = setTimeout(() => {
      setSuccess(null);
      successTimeoutRef.current = null;
    }, SUCCESS_MESSAGE_DURATION_MS);
  };

  const handleRevokeConfirm = async () => {
    if (!sessionToRevoke) return;

    // Debounce: prevent rapid-fire requests
    const now = Date.now();
    if (now - lastRevokeTimeRef.current < REVOKE_DEBOUNCE_MS) {
      return;
    }
    lastRevokeTimeRef.current = now;

    setRevoking(true);
    setError(null);
    setSuccess(null);

    try {
      await client.revokeSession(sessionToRevoke);
      setShowRevokeModal(false);
      setSessionToRevoke(null);
      setSuccessWithTimeout('Session revoked successfully');
      await loadSessions();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to revoke session');
    } finally {
      setRevoking(false);
    }
  };

  const handleRevokeAllClick = () => {
    setShowRevokeAllModal(true);
  };

  const handleRevokeAllConfirm = async () => {
    // Debounce: prevent rapid-fire requests
    const now = Date.now();
    if (now - lastRevokeTimeRef.current < REVOKE_DEBOUNCE_MS) {
      return;
    }
    lastRevokeTimeRef.current = now;

    setRevoking(true);
    setError(null);
    setSuccess(null);

    try {
      await client.revokeAllSessions();
      setShowRevokeAllModal(false);
      setSuccessWithTimeout('All other sessions revoked successfully');
      await loadSessions();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to revoke all sessions');
    } finally {
      setRevoking(false);
    }
  };

  const nonCurrentSessions = sessions.filter((s) => !s.is_current);
  const hasOtherSessions = nonCurrentSessions.length > 0;

  if (loading) {
    return (
      <div className="page-container">
        <div className="page-header">
          <h1 className="page-title">Active Sessions</h1>
        </div>
        <div className="loading-state">Loading sessions...</div>
      </div>
    );
  }

  return (
    <div className="page-container">
      <div className="page-header">
        <h1 className="page-title">Active Sessions</h1>
        <p className="page-description">
          Manage your active sessions across all devices. You can revoke access to any session at
          any time.
        </p>
      </div>

      {error && (
        <div className="alert alert-error" role="alert" aria-live="assertive">
          {error}
          <Button
            onClick={loadSessions}
            disabled={loading}
            variant="secondary"
            size="sm"
            style={{ marginLeft: '1rem' }}
          >
            Retry
          </Button>
        </div>
      )}
      {success && (
        <div className="alert alert-success" role="status" aria-live="polite">
          {success}
        </div>
      )}

      <div className="sessions-container">
        {sessions.length === 0 ? (
          <div className="empty-state">
            <p>No active sessions found.</p>
          </div>
        ) : (
          <>
            <div className="sessions-table-wrapper">
              <table className="sessions-table" aria-label="Active sessions">
                <thead>
                  <tr>
                    <th>Device / Browser</th>
                    <th>Location</th>
                    <th>IP Address</th>
                    <th>Last Active</th>
                    <th>Created</th>
                    <th>Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {sessions.map((session) => (
                    <tr key={session.id} className={session.is_current ? 'current-session' : ''}>
                      <td>
                        <div className="device-info">
                          {session.device_info || parseUserAgent(session.user_agent)}
                          {session.is_current && (
                            <span className="current-badge">Current Session</span>
                          )}
                        </div>
                      </td>
                      <td>{session.location || 'Unknown'}</td>
                      <td className="ip-address">{session.ip_address || 'Unknown'}</td>
                      <td className="timestamp">{formatRelativeTime(session.last_active_at)}</td>
                      <td className="timestamp">{formatRelativeTime(session.created_at)}</td>
                      <td>
                        <Button
                          onClick={() => handleRevokeClick(session.id)}
                          disabled={session.is_current || revoking}
                          variant="secondary"
                          size="sm"
                          aria-label={
                            session.is_current
                              ? 'Cannot revoke current session'
                              : `Revoke session from ${session.device_info || parseUserAgent(session.user_agent)}`
                          }
                        >
                          Revoke
                        </Button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>

            <div className="sessions-actions">
              <Button
                onClick={handleRevokeAllClick}
                disabled={!hasOtherSessions || revoking}
                variant="danger"
              >
                Revoke All Other Sessions
              </Button>
            </div>
          </>
        )}
      </div>

      {/* Revoke Single Session Modal */}
      <Modal
        isOpen={showRevokeModal}
        onClose={() => {
          if (!revoking) {
            setShowRevokeModal(false);
            setSessionToRevoke(null);
          }
        }}
        title="Revoke Session"
      >
        <p>Are you sure you want to revoke this session? This action cannot be undone.</p>
        <div className="modal-actions">
          <Button
            onClick={() => {
              setShowRevokeModal(false);
              setSessionToRevoke(null);
            }}
            disabled={revoking}
            variant="secondary"
          >
            Cancel
          </Button>
          <Button onClick={handleRevokeConfirm} loading={revoking} variant="danger">
            Confirm
          </Button>
        </div>
      </Modal>

      {/* Revoke All Sessions Modal */}
      <Modal
        isOpen={showRevokeAllModal}
        onClose={() => {
          if (!revoking) {
            setShowRevokeAllModal(false);
          }
        }}
        title="Revoke All Other Sessions"
      >
        <p>
          Are you sure you want to revoke all other sessions? This will sign you out from all
          devices except this one. This action cannot be undone.
        </p>
        <div className="modal-actions">
          <Button
            onClick={() => setShowRevokeAllModal(false)}
            disabled={revoking}
            variant="secondary"
          >
            Cancel
          </Button>
          <Button onClick={handleRevokeAllConfirm} loading={revoking} variant="danger">
            Confirm
          </Button>
        </div>
      </Modal>
    </div>
  );
}
