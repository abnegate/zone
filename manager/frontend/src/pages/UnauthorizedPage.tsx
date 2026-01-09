import { Link } from 'react-router-dom';
import '../features/auth/pages/AuthPage.css';

export default function UnauthorizedPage() {
  return (
    <div className="auth-page">
      <div className="auth-container">
        <div className="auth-header">
          <h1
            style={{
              background: 'linear-gradient(135deg, #ef4444, #f59e0b)',
              WebkitBackgroundClip: 'text',
              WebkitTextFillColor: 'transparent',
            }}
          >
            Access Denied
          </h1>
          <p>You don't have permission to access this page</p>
        </div>

        <div className="auth-footer" style={{ borderTop: 'none', marginTop: 0, paddingTop: 0 }}>
          <Link to="/" className="btn btn-primary btn-block">
            Go to Home
          </Link>
        </div>
      </div>
    </div>
  );
}
