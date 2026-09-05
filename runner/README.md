# Zone

AI-powered development platform with intelligent context gathering, task automation, and knowledge management.

## Vision

Zone is a comprehensive AI development assistant that combines:

### 1. Chat
- AI chat with knowledge base context injection
- Message embeddings for conversation memory and search
- Configurable scope: organization, workspace, project, or user level
- Real-time streaming responses via WebSocket

### 2. Projects
- Repository mapping with hierarchical organization
- **Organization** → **Workspace** → **Project** structure
- Project-level AI settings and configurations
- Integration with GitHub, GitLab, and local repositories

### 3. Tasks
- Project-linked tasks with agent loop execution
- GitHub and Linear issue synchronization
- Real-time task progress via WebSocket
- Automatic PR creation on task completion
- Tool execution through sandboxed runner
- MCP servers (magents by default) so a task can spawn or message other coding agents

### 4. Sources
- External data source connections for context gathering
- Supported adapters:
  - **GitHub** - Repository files, issues, PRs
  - **GitLab** - Repository files, merge requests
  - **Filesystem** - Local files and directories
  - **Web** - Web page scraping with SSRF protection
  - **iCal** - Calendar events
  - **Text** - Raw text content
  - **Notion** - (planned)
  - **Slack** - (planned)
  - **Discord** - (planned)
  - **IMAP** - (planned)

### 5. Knowledge Base
- Visual documentation from chat/project/task history
- Manual data entry and web link ingestion
- Semantic search across all knowledge
- Automatic embedding generation for retrieval

## Architecture

```
runner/
├── zone_server/     # REST API + WebSocket server
├── zone_core/       # Agent loop, LLM client, tools
├── zone_context/    # Context gathering, embeddings, heuristics
├── zone_runner/     # CLI daemon/one-shot execution
├── zone_cli/        # User-facing CLI tool
├── zone_installer/  # Desktop/mobile client HTTP shell
├── zone_desktop/    # Tauri desktop, Android, and iOS client
└── tool_runner/     # Sandboxed command execution
```

### Data Flow

```
User Request
    ↓
zone_server (REST/WebSocket)
    ↓
zone_core (Agent Loop)
    ↓
├── LLM Client (Claude, GPT, etc.)
├── Tool Execution (via tool_runner + MCP servers such as magents)
└── Context Injection (via zone_context)
    ↓
zone_context
    ├── Source Adapters (GitHub, GitLab, Web, etc.)
    ├── Embedding Service (Ollama, OpenAI, Bedrock)
    └── pgvector Storage (PostgreSQL)
```

## Getting Started

### Prerequisites

- Rust 1.97.1+
- PostgreSQL 15+ with pgvector extension
- Redis 7+
- Docker (optional, for development)

### Development Setup

```bash
# Clone the repository
git clone https://github.com/zone-dev/zone.git
cd zone/runner

# Set up environment
cp .env.example .env
# Edit .env with your database and API credentials

# Run migrations
sqlx database create
sqlx migrate run

# Build all crates
cargo build

# Run tests
cargo test

# Start the server
cargo run --bin zone_server
```

### Running the CLI

```bash
# Install the CLI
cargo install --path zone_cli

# Login
zone login

# Start a chat
zone chat "What files are in this repository?"

# Run a task
zone task run "Add error handling to the API endpoints"
```

## Configuration

### AI Settings

Zone supports multiple AI providers configurable at organization, workspace, or project level:

- **Chat Model**: Claude, GPT-4, local models via LiteLLM
- **Reasoning Model**: o1, Claude with extended thinking
- **Embedding Model**: text-embedding-3-small, nomic-embed, local models

### Environment Variables

```bash
DATABASE_URL=postgres://user:pass@localhost/zone
REDIS_URL=redis://localhost:6379
JWT_SECRET=your-secret-key
ENCRYPTION_KEY=32-byte-hex-key-for-credentials
```

## API Reference

See [API Documentation](./docs/api.md) for full endpoint reference.

### Key Endpoints

- `POST /api/chats` - Create chat session
- `POST /api/tasks` - Create and run tasks
- `POST /api/sources` - Add data sources
- `POST /api/context/gather` - Trigger context gathering
- `GET /api/context/search` - Semantic search
- `WS /ws/chat/{id}` - Chat streaming
- `WS /ws/task/{id}` - Task progress streaming

## License

MIT License - See [LICENSE](./LICENSE) for details.
