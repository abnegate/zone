import './StubPage.css';

export default function ChatsPage() {
  return (
    <div className="stub-page">
      <header className="page-header">
        <h1>Chats</h1>
        <p className="subtitle">Conversations with your AI models</p>
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
            <path d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
          </svg>
        </div>
        <h2>Conversations</h2>
        <p className="stub-description">
          Have conversations with your installed models. All chats contribute to the Wiki knowledge
          base, helping models learn and improve over time.
        </p>
        <div className="stub-features">
          <div className="stub-feature">
            <span className="stub-feature-icon">1</span>
            <span>Select from installed models</span>
          </div>
          <div className="stub-feature">
            <span className="stub-feature-icon">2</span>
            <span>Multi-turn conversations with context</span>
          </div>
          <div className="stub-feature">
            <span className="stub-feature-icon">3</span>
            <span>Automatic knowledge extraction to Wiki</span>
          </div>
        </div>
      </section>
    </div>
  );
}
