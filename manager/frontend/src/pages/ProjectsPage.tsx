import './StubPage.css';

export default function ProjectsPage() {
  return (
    <div className="stub-page">
      <header className="page-header">
        <h1>Projects</h1>
        <p className="subtitle">Organize work with GitHub integration</p>
      </header>

      <section className="card stub-content">
        <div className="stub-icon">
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <path d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
          </svg>
        </div>
        <h2>Project Management</h2>
        <p className="stub-description">
          Projects contain tasks and can optionally be linked to GitHub repositories. Track status
          as Active, On Hold, or Cancelled. Each project groups related work together for
          autonomous agent execution.
        </p>
        <div className="stub-features">
          <div className="stub-feature">
            <span className="stub-feature-icon">1</span>
            <span>Active, On Hold, Cancelled status tracking</span>
          </div>
          <div className="stub-feature">
            <span className="stub-feature-icon">2</span>
            <span>Optional GitHub repository linking</span>
          </div>
          <div className="stub-feature">
            <span className="stub-feature-icon">3</span>
            <span>Contains tasks for agent execution</span>
          </div>
        </div>
      </section>
    </div>
  );
}
