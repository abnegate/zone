import { Link } from 'react-router-dom';
import '../features/auth/pages/AuthPage.css';

export default function UnauthorizedPage() {
  return (
    <div className="auth-page">
      <div className="auth-container">
        <div className="auth-header">
          <h1>Access Denied</h1>
          <p>You don't have permission to access this page</p>
        </div>

        <div className="auth-footer auth-footer--plain">
          <Link to="/" className="btn btn-primary btn-lg btn-block">
            Go to Home
          </Link>
        </div>
      </div>
    </div>
  );
}
