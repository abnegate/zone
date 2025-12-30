import './StubPage.css';

export default function WikiPage() {
  return (
    <div className="stub-page">
      <header className="page-header">
        <h1>Wiki</h1>
        <p className="subtitle">Knowledge base for your AI models</p>
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
            <path d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
          </svg>
        </div>
        <h2>Knowledge Base</h2>
        <p className="stub-description">
          A growing knowledge base that learns from your conversations and can be intentionally fed
          content, links, and documentation. Models can access this knowledge to provide better,
          more contextual responses.
        </p>
        <div className="stub-features">
          <div className="stub-feature">
            <span className="stub-feature-icon">1</span>
            <span>Auto-populated from chat conversations</span>
          </div>
          <div className="stub-feature">
            <span className="stub-feature-icon">2</span>
            <span>Import docs, links, and content</span>
          </div>
          <div className="stub-feature">
            <span className="stub-feature-icon">3</span>
            <span>Models learn from the knowledge base</span>
          </div>
        </div>
      </section>
    </div>
  );
}
