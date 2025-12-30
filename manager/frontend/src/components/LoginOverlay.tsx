import { type FormEvent, useState } from 'react';
import { useAuth } from '../context/AuthContext';
import './LoginOverlay.css';

export default function LoginOverlay() {
  const { login, isAuthenticated } = useAuth();
  const [apiKey, setApiKey] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  if (isAuthenticated) {
    return null;
  }

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (!apiKey.trim()) {
      setError('Please enter an API key');
      return;
    }

    setLoading(true);
    setError('');

    const success = await login(apiKey.trim());
    setLoading(false);

    if (!success) {
      setError('Invalid API key');
    }
  };

  return (
    <div className="login-overlay">
      <div className="login-modal">
        <h2>Zone</h2>
        <p className="help-text">Enter your API key to access the manager</p>

        <form className="login-form" onSubmit={handleSubmit}>
          <input
            type="password"
            placeholder="API Key"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            disabled={loading}
            autoFocus
          />

          {error && <div className="login-error">{error}</div>}

          <button type="submit" className="btn btn-primary" disabled={loading}>
            {loading ? (
              <>
                <span className="spinner" />
                Authenticating...
              </>
            ) : (
              'Login'
            )}
          </button>
        </form>
      </div>
    </div>
  );
}
