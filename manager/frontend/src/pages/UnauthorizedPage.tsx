import { Link } from 'react-router-dom';
import './UnauthorizedPage.css';

export default function UnauthorizedPage() {
  return (
    <div className="unauthorized-page">
      <div className="unauthorized-card">
        <svg
          className="unauthorized-icon"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <rect x="4" y="10" width="16" height="11" rx="2" />
          <path d="M8 10V7a4 4 0 018 0v3" />
          <path d="M12 14v3" />
        </svg>
        <h1 className="unauthorized-title">Access Denied</h1>
        <p className="unauthorized-text">You don't have permission to access this page</p>
        <Link to="/" className="btn btn-primary">
          Go to Home
        </Link>
      </div>
    </div>
  );
}
