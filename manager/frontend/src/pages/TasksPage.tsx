import './StubPage.css';

export default function TasksPage() {
  return (
    <div className="stub-page">
      <header className="page-header">
        <h1>Tasks</h1>
        <p className="subtitle">Autonomous agent workflows</p>
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
            <path d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-6 9l2 2 4-4" />
          </svg>
        </div>
        <h2>Agent Task Execution</h2>
        <p className="stub-description">
          Define tasks within projects and let autonomous agents complete them. Each task runs
          through a structured workflow with multiple specialized agents working together.
        </p>
        <div className="stub-features">
          <div className="stub-feature">
            <span className="stub-feature-icon">1</span>
            <span>Architect plans the implementation</span>
          </div>
          <div className="stub-feature">
            <span className="stub-feature-icon">2</span>
            <span>Developer writes tests then implements</span>
          </div>
          <div className="stub-feature">
            <span className="stub-feature-icon">3</span>
            <span>Griller reviews, Developer fixes</span>
          </div>
          <div className="stub-feature">
            <span className="stub-feature-icon">4</span>
            <span>Architect approves final implementation</span>
          </div>
        </div>
      </section>
    </div>
  );
}
