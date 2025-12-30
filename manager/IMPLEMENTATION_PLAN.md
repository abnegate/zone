# Zone Manager Implementation Plan

## Executive Summary

This document outlines the complete implementation plan for Zone Manager, an AI orchestration platform that enables autonomous software development through coordinated multi-agent workflows. The system builds upon the existing Ollama model management infrastructure to add Projects, Tasks, Chats, and Wiki capabilities.

**Current State:**
- Working Models page (browse, install, delete models via Ollama)
- Gleam backend with API authentication
- React frontend with routing and auth context
- Postgres database available (currently used by LiteLLM)
- Stub pages for Chats, Projects, Tasks, Wiki

**Target State:**
- Full chat system with model selection and conversation history
- Project management with GitHub integration
- Task lifecycle with multi-agent execution engine
- Wiki knowledge base with auto-extraction and manual ingestion
- Real-time progress monitoring via WebSocket/SSE

---

## 1. Data Models & Database Schema

### 1.1 Database Choice: PostgreSQL + pgvector

**Rationale:**
- Postgres already available in stack (used by LiteLLM)
- pgvector extension for Wiki embeddings
- Strong ACID guarantees for task state management
- JSON support for flexible metadata storage

### 1.2 Core Entities

#### Chats & Messages
```sql
CREATE TABLE chats (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  title TEXT NOT NULL,
  model_name TEXT NOT NULL,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  archived BOOLEAN DEFAULT FALSE
);

CREATE TABLE messages (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  chat_id UUID NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
  role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system')),
  content TEXT NOT NULL,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  metadata JSONB DEFAULT '{}'::jsonb,
  INDEX idx_messages_chat_id (chat_id),
  INDEX idx_messages_created_at (created_at)
);
```

#### Projects
```sql
CREATE TABLE projects (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name TEXT NOT NULL,
  description TEXT,
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'on_hold', 'cancelled')),
  github_repo_url TEXT,
  github_access_token TEXT, -- encrypted at app level
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE INDEX idx_projects_status ON projects(status);
```

#### Tasks
```sql
CREATE TABLE tasks (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  description TEXT NOT NULL,
  acceptance_criteria TEXT,
  status TEXT NOT NULL DEFAULT 'created' CHECK (status IN
    ('created', 'queued', 'in_progress', 'review', 'complete', 'blocked')),
  priority INTEGER DEFAULT 3 CHECK (priority >= 1 AND priority <= 5),
  model_name TEXT, -- NULL = use default
  dependencies JSONB DEFAULT '[]'::jsonb, -- array of task IDs
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  started_at TIMESTAMP WITH TIME ZONE,
  completed_at TIMESTAMP WITH TIME ZONE,
  INDEX idx_tasks_project_id (project_id),
  INDEX idx_tasks_status (status)
);

CREATE TABLE task_runs (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  status TEXT NOT NULL CHECK (status IN ('running', 'completed', 'failed', 'cancelled')),
  current_phase TEXT, -- architect, developer_tests, developer_impl, etc.
  progress_percent INTEGER DEFAULT 0 CHECK (progress_percent >= 0 AND progress_percent <= 100),
  started_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  completed_at TIMESTAMP WITH TIME ZONE,
  error_message TEXT,
  artifacts JSONB DEFAULT '{}'::jsonb, -- file paths, git commits, etc.
  INDEX idx_task_runs_task_id (task_id),
  INDEX idx_task_runs_status (status)
);

CREATE TABLE task_run_logs (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  task_run_id UUID NOT NULL REFERENCES task_runs(id) ON DELETE CASCADE,
  phase TEXT NOT NULL,
  agent_type TEXT NOT NULL, -- architect, developer, griller
  log_level TEXT NOT NULL CHECK (log_level IN ('debug', 'info', 'warning', 'error')),
  message TEXT NOT NULL,
  metadata JSONB DEFAULT '{}'::jsonb,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  INDEX idx_task_run_logs_run_id (task_run_id),
  INDEX idx_task_run_logs_created_at (created_at)
);
```

#### Wiki Knowledge Base
```sql
-- Install pgvector extension
CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE wiki_entries (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  title TEXT NOT NULL,
  content TEXT NOT NULL,
  source_type TEXT NOT NULL CHECK (source_type IN
    ('chat', 'manual', 'url', 'task', 'github')),
  source_id UUID, -- references chat_id, task_id, etc.
  source_url TEXT,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  metadata JSONB DEFAULT '{}'::jsonb,
  INDEX idx_wiki_entries_source_type (source_type)
);

CREATE TABLE wiki_chunks (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  wiki_entry_id UUID NOT NULL REFERENCES wiki_entries(id) ON DELETE CASCADE,
  chunk_index INTEGER NOT NULL,
  content TEXT NOT NULL,
  embedding vector(1024), -- dimension depends on embedding model
  token_count INTEGER,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  INDEX idx_wiki_chunks_entry_id (wiki_entry_id)
);

-- Vector similarity search index
CREATE INDEX idx_wiki_chunks_embedding ON wiki_chunks
  USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100);
```

### 1.3 Database Migrations Strategy

**Approach:** Use Gleam's SQL file loading + manual migration tracking
- Create `/home/jakeb/code/voiz/manager/migrations/` directory
- Number migrations: `001_initial_schema.sql`, `002_add_wiki.sql`, etc.
- Track applied migrations in `schema_migrations` table
- Run migrations on container startup

---

## 2. Backend API (Gleam)

### 2.1 Project Structure

```
manager/src/
├── manager.gleam              # Main HTTP server (existing)
├── database/
│   ├── connection.gleam       # Postgres connection pool
│   ├── migrations.gleam       # Migration runner
│   └── queries/
│       ├── chats.gleam        # Chat CRUD operations
│       ├── projects.gleam     # Project CRUD operations
│       ├── tasks.gleam        # Task CRUD operations
│       └── wiki.gleam         # Wiki CRUD operations
├── api/
│   ├── chats_routes.gleam     # Chat endpoints
│   ├── projects_routes.gleam  # Project endpoints
│   ├── tasks_routes.gleam     # Task endpoints
│   └── wiki_routes.gleam      # Wiki endpoints
├── agents/
│   ├── executor.gleam         # Task execution orchestrator
│   ├── architect.gleam        # Architect agent
│   ├── developer.gleam        # Developer agent
│   ├── griller.gleam          # Code review agent
│   └── prompts.gleam          # Agent system prompts
├── github/
│   ├── client.gleam           # GitHub API client
│   └── operations.gleam       # Clone, read, write, commit
├── wiki/
│   ├── extractor.gleam        # Extract knowledge from chats
│   ├── embedder.gleam         # Generate embeddings
│   ├── chunker.gleam          # Split content into chunks
│   └── search.gleam           # Vector similarity search
└── models/
    ├── chat.gleam             # Chat types
    ├── project.gleam          # Project types
    ├── task.gleam             # Task types
    └── wiki.gleam             # Wiki types
```

### 2.2 API Endpoints

#### Chats API
```
GET    /api/chats                    # List all chats (paginated)
POST   /api/chats                    # Create new chat
GET    /api/chats/:id                # Get chat with messages
DELETE /api/chats/:id                # Delete chat
POST   /api/chats/:id/messages       # Add message & get response
PUT    /api/chats/:id/title          # Update chat title
PUT    /api/chats/:id/archive        # Archive/unarchive chat
```

**Implementation Details:**
- Chat creation includes model selection
- Message streaming via SSE or chunked response
- Auto-title generation after first exchange
- Extract key facts to Wiki asynchronously

#### Projects API
```
GET    /api/projects                 # List all projects
POST   /api/projects                 # Create new project
GET    /api/projects/:id             # Get project details
PUT    /api/projects/:id             # Update project
DELETE /api/projects/:id             # Delete project
POST   /api/projects/:id/github/link # Link GitHub repository
DELETE /api/projects/:id/github      # Unlink GitHub repository
GET    /api/projects/:id/tasks       # List project tasks
```

**Implementation Details:**
- Status filtering (active/on_hold/cancelled)
- GitHub token encryption using project-level secrets
- Validate GitHub access on linking

#### Tasks API
```
GET    /api/tasks                    # List tasks (with filters)
POST   /api/tasks                    # Create new task
GET    /api/tasks/:id                # Get task details
PUT    /api/tasks/:id                # Update task
DELETE /api/tasks/:id                # Delete task
POST   /api/tasks/:id/start          # Start task execution
POST   /api/tasks/:id/stop           # Cancel running task
GET    /api/tasks/:id/runs           # List task runs
GET    /api/tasks/:id/runs/:run_id   # Get run details
WS     /ws/tasks/:run_id             # Real-time progress stream
GET    /api/tasks/:id/runs/:run_id/logs # Get execution logs
```

**Implementation Details:**
- Dependency validation on creation
- Status transitions: created -> queued -> in_progress -> review -> complete
- Block task if dependencies incomplete
- Progress streaming via WebSocket

#### Wiki API
```
GET    /api/wiki/search              # Search knowledge base
POST   /api/wiki/ingest              # Manually add content
GET    /api/wiki/entries             # List entries (paginated)
GET    /api/wiki/entries/:id         # Get entry details
DELETE /api/wiki/entries/:id         # Delete entry
POST   /api/wiki/ingest/url          # Ingest from URL
POST   /api/wiki/extract/:chat_id    # Manually trigger chat extraction
```

**Implementation Details:**
- Search uses vector similarity + keyword filtering
- URL ingestion: fetch, parse, chunk, embed
- Chat extraction: summarize conversations, extract facts
- Return top-k chunks with relevance scores

### 2.3 Database Layer (Gleam Libraries)

**Required Dependencies:**
```toml
# gleam.toml additions
[dependencies]
pgo = "~> 0.14"          # PostgreSQL driver
decode = "~> 0.10"       # JSON decoding
gleam_json = "~> 1.0"    # JSON encoding
gleam_crypto = "~> 1.3"  # For encryption
gleam_erlang = "~> 0.25" # Process management for agents
```

**Connection Pool:**
```gleam
// database/connection.gleam
import pgo

pub fn start() -> Result(pgo.Connection, String) {
  let config = pgo.Config(
    ..pgo.default_config(),
    host: "postgres",
    database: "voiz_manager",
    user: get_env("POSTGRES_USER"),
    password: get_env("POSTGRES_PASSWORD"),
    pool_size: 10,
  )

  pgo.connect(config)
}
```

---

## 3. Agent Executor System

### 3.1 Architecture

**Execution Model:**
- Each task execution spawns a supervisor process
- Supervisor coordinates agent phases sequentially
- Each agent phase communicates via LiteLLM
- Progress updates published to Subject for WebSocket streaming
- Logs persisted to `task_run_logs` table

**Phase Flow:**
```
1. Architect (Planning)
   - Analyzes requirements
   - Creates implementation plan
   - Defines subtasks
   - Output: plan.md artifact

2. Developer (Tests)
   - Writes test cases
   - Establishes coverage goals
   - Output: test files

3. Developer (Implementation)
   - Implements feature
   - Ensures tests pass
   - Output: implementation files

4. Griller (Code Review)
   - Reviews implementation
   - Identifies issues
   - Provides feedback
   - Output: review.md artifact

5. Developer (Fixes)
   - Addresses review feedback
   - Refines implementation
   - Output: updated files

6. Architect (Final Review)
   - Validates against plan
   - Checks architecture
   - Output: approval or changes needed

7. Developer (Final Touches)
   - Final adjustments
   - Documentation
   - Output: completed task
```

### 3.2 Implementation

#### Agent System Prompts
```gleam
// agents/prompts.gleam
pub fn architect_planning_prompt(task: Task, project: Project) -> String {
  "You are an expert software architect...
   Task: " <> task.description <> "
   Project: " <> project.name <> "
   ..."
}

pub fn developer_test_prompt(task: Task, plan: String) -> String { ... }
pub fn developer_impl_prompt(task: Task, tests: String) -> String { ... }
pub fn griller_review_prompt(task: Task, impl: String) -> String { ... }
```

#### Task Executor
```gleam
// agents/executor.gleam
import gleam/erlang/process
import gleam/otp/actor

pub type ExecutorMessage {
  Start(task_id: String, run_id: String)
  Stop(run_id: String)
  Progress(phase: String, percent: Int, message: String)
}

pub fn start_task_execution(
  task: Task,
  project: Project,
  progress_subject: Subject(Progress)
) -> Result(Nil, String) {
  // 1. Create task_run record
  let run = create_task_run(task.id)

  // 2. Spawn supervised process
  let assert Ok(executor) = actor.start(run.id, fn(msg, state) {
    case msg {
      Progress(phase, pct, msg) -> {
        // Update database
        update_run_progress(run.id, phase, pct)
        // Publish to WebSocket
        process.send(progress_subject, Progress(phase, pct, msg))
        actor.continue(state)
      }
    }
  })

  // 3. Execute phases sequentially
  execute_phase_architect(task, project, executor)
  execute_phase_developer_tests(task, project, executor)
  execute_phase_developer_impl(task, project, executor)
  execute_phase_griller(task, project, executor)
  execute_phase_developer_fixes(task, project, executor)
  execute_phase_architect_review(task, project, executor)
  execute_phase_developer_final(task, project, executor)

  // 4. Complete task
  complete_task_run(run.id)
  Ok(Nil)
}
```

#### GitHub Integration
```gleam
// github/operations.gleam
pub fn clone_repository(url: String, token: String, target_dir: String) -> Result(Nil, String)
pub fn read_file(repo_path: String, file_path: String) -> Result(String, String)
pub fn write_file(repo_path: String, file_path: String, content: String) -> Result(Nil, String)
pub fn commit_changes(repo_path: String, message: String) -> Result(String, String)
pub fn create_pull_request(repo: String, branch: String, title: String, body: String) -> Result(String, String)
```

**Implementation Notes:**
- Use `gleam/os` for git command execution
- Clone to temporary directory: `/tmp/repos/{project_id}/{task_id}/`
- Create feature branch: `task/{task_id}`
- Commit incrementally during agent phases
- Push at completion or on request

### 3.3 WebSocket Progress Streaming

```gleam
// Similar to existing /ws/pull endpoint
WS /ws/tasks/:run_id?api_key=xxx

// Message format:
{
  "type": "progress",
  "run_id": "uuid",
  "phase": "developer_impl",
  "progress_percent": 45,
  "message": "Implementing feature X..."
}

{
  "type": "log",
  "run_id": "uuid",
  "phase": "griller",
  "level": "info",
  "message": "Reviewing code..."
}

{
  "type": "complete",
  "run_id": "uuid",
  "success": true,
  "message": "Task completed successfully"
}
```

---

## 4. Frontend Implementation

### 4.1 Type Definitions

```typescript
// manager/frontend/src/types/index.ts additions

// Chats
export interface Chat {
  id: string;
  title: string;
  model_name: string;
  created_at: string;
  updated_at: string;
  archived: boolean;
}

export interface Message {
  id: string;
  chat_id: string;
  role: 'user' | 'assistant' | 'system';
  content: string;
  created_at: string;
  metadata?: Record<string, any>;
}

export interface ChatWithMessages extends Chat {
  messages: Message[];
}

// Projects
export interface Project {
  id: string;
  name: string;
  description: string;
  status: 'active' | 'on_hold' | 'cancelled';
  github_repo_url?: string;
  created_at: string;
  updated_at: string;
}

// Tasks
export type TaskStatus = 'created' | 'queued' | 'in_progress' | 'review' | 'complete' | 'blocked';

export interface Task {
  id: string;
  project_id: string;
  title: string;
  description: string;
  acceptance_criteria?: string;
  status: TaskStatus;
  priority: 1 | 2 | 3 | 4 | 5;
  model_name?: string;
  dependencies: string[];
  created_at: string;
  updated_at: string;
  started_at?: string;
  completed_at?: string;
}

export interface TaskRun {
  id: string;
  task_id: string;
  status: 'running' | 'completed' | 'failed' | 'cancelled';
  current_phase?: string;
  progress_percent: number;
  started_at: string;
  completed_at?: string;
  error_message?: string;
  artifacts?: Record<string, any>;
}

export interface TaskRunLog {
  id: string;
  task_run_id: string;
  phase: string;
  agent_type: string;
  log_level: 'debug' | 'info' | 'warning' | 'error';
  message: string;
  metadata?: Record<string, any>;
  created_at: string;
}

// Wiki
export interface WikiEntry {
  id: string;
  title: string;
  content: string;
  source_type: 'chat' | 'manual' | 'url' | 'task' | 'github';
  source_id?: string;
  source_url?: string;
  created_at: string;
  updated_at: string;
  metadata?: Record<string, any>;
}

export interface WikiSearchResult {
  entry: WikiEntry;
  chunk: string;
  relevance_score: number;
}
```

### 4.2 Chats Page Implementation

**Features:**
- Sidebar: List of chats (most recent first)
- Main area: Chat messages with markdown rendering
- Input: Message composer with model selection
- New chat button
- Archive/delete chat actions

**Components:**
```
ChatsPage.tsx
├── ChatList.tsx           # Sidebar list
├── ChatView.tsx           # Message display area
├── MessageComposer.tsx    # Input + send
└── ModelSelector.tsx      # Dropdown for model selection
```

**API Client Methods:**
```typescript
// api/client.ts additions
async listChats(archived = false): Promise<Chat[]>
async createChat(modelName: string, firstMessage: string): Promise<ChatWithMessages>
async getChat(id: string): Promise<ChatWithMessages>
async sendMessage(chatId: string, content: string): Promise<Message>
async updateChatTitle(chatId: string, title: string): Promise<void>
async archiveChat(chatId: string): Promise<void>
async deleteChat(chatId: string): Promise<void>
```

**Hooks:**
```typescript
// hooks/useChats.ts
export function useChats() {
  const [chats, setChats] = useState<Chat[]>([]);
  const [loading, setLoading] = useState(false);
  // ...
}

// hooks/useChat.ts
export function useChat(chatId: string) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [sending, setSending] = useState(false);
  // ...
}
```

### 4.3 Projects Page Implementation

**Features:**
- Card grid: Display projects with status badges
- Create project modal: Name, description, status
- Edit project modal: Update details, link GitHub
- Delete confirmation
- Click to view project tasks

**Components:**
```
ProjectsPage.tsx
├── ProjectCard.tsx        # Individual project card
├── ProjectModal.tsx       # Create/edit modal
├── GitHubLinkForm.tsx     # GitHub integration form
└── StatusBadge.tsx        # Colored status indicator
```

**API Client Methods:**
```typescript
async listProjects(): Promise<Project[]>
async createProject(data: CreateProjectRequest): Promise<Project>
async updateProject(id: string, data: UpdateProjectRequest): Promise<Project>
async deleteProject(id: string): Promise<void>
async linkGitHub(id: string, repoUrl: string, token: string): Promise<void>
async unlinkGitHub(id: string): Promise<void>
```

### 4.4 Tasks Page Implementation

**Features:**
- Kanban board view (columns: Created, Queued, In Progress, Review, Complete, Blocked)
- List view with filtering
- Create task modal with project selection
- Task detail view with run history
- Start/stop task execution
- Real-time progress monitoring via WebSocket
- View logs for task runs

**Components:**
```
TasksPage.tsx
├── TaskBoard.tsx          # Kanban board layout
├── TaskColumn.tsx         # Column in kanban
├── TaskCard.tsx           # Task card in kanban
├── TaskList.tsx           # Alternative list view
├── TaskModal.tsx          # Create/edit task
├── TaskDetailModal.tsx    # View task details & runs
├── TaskProgressView.tsx   # Live progress display
└── TaskLogsViewer.tsx     # Execution logs
```

**WebSocket Hook:**
```typescript
// hooks/useTaskProgress.ts
export function useTaskProgress(runId: string) {
  const [progress, setProgress] = useState<number>(0);
  const [phase, setPhase] = useState<string>('');
  const [logs, setLogs] = useState<TaskRunLog[]>([]);
  const [status, setStatus] = useState<'running' | 'completed' | 'failed'>('running');

  useEffect(() => {
    const apiKey = getApiKey();
    const ws = new WebSocket(`ws://manager.${domain}/ws/tasks/${runId}?api_key=${apiKey}`);

    ws.onmessage = (event) => {
      const data = JSON.parse(event.data);
      if (data.type === 'progress') {
        setProgress(data.progress_percent);
        setPhase(data.phase);
      } else if (data.type === 'log') {
        setLogs(prev => [...prev, data]);
      } else if (data.type === 'complete') {
        setStatus(data.success ? 'completed' : 'failed');
      }
    };

    return () => ws.close();
  }, [runId]);

  return { progress, phase, logs, status };
}
```

### 4.5 Wiki Page Implementation

**Features:**
- Search bar with results list
- Manual content ingestion (text, URL)
- Browse entries by source type
- View entry details with chunks
- Delete entries

**Components:**
```
WikiPage.tsx
├── WikiSearch.tsx         # Search input & results
├── WikiIngestForm.tsx     # Add content manually
├── WikiEntryList.tsx      # Browse all entries
├── WikiEntryCard.tsx      # Entry display card
└── WikiChunkViewer.tsx    # View chunks & embeddings
```

**API Client Methods:**
```typescript
async searchWiki(query: string): Promise<WikiSearchResult[]>
async ingestContent(title: string, content: string): Promise<WikiEntry>
async ingestUrl(url: string): Promise<WikiEntry>
async listWikiEntries(sourceType?: string): Promise<WikiEntry[]>
async getWikiEntry(id: string): Promise<WikiEntry>
async deleteWikiEntry(id: string): Promise<void>
```

---

## 5. Infrastructure & Dependencies

### 5.1 Docker Compose Updates

```yaml
# docker-compose.yml additions

services:
  postgres:
    # Add pgvector support
    image: pgvector/pgvector:${DOCKER_VERSION_POSTGRES:-pg16}
    # ... existing config
    environment:
      - POSTGRES_DB=voiz_manager  # Change from generic DB

  manager:
    # ... existing config
    environment:
      - POSTGRES_HOST=postgres
      - POSTGRES_DB=voiz_manager
      - POSTGRES_USER=${POSTGRES_USER}
      - POSTGRES_PASSWORD=${POSTGRES_PASSWORD}
      - GITHUB_APP_PRIVATE_KEY=${GITHUB_APP_PRIVATE_KEY}  # For GitHub integration
    volumes:
      - manager_repos:/tmp/repos  # For cloned repositories
      - manager_artifacts:/app/artifacts  # Task outputs

volumes:
  manager_repos:
    name: voiz_manager_repos
  manager_artifacts:
    name: voiz_manager_artifacts
```

### 5.2 Required Gleam Dependencies

```toml
# manager/gleam.toml
[dependencies]
# Existing
wisp = "~> 0.16"
gleam_stdlib = "~> 0.43"
gleam_json = "~> 1.0"
gleam_http = "~> 3.6"
gleam_httpc = "~> 0.3"
gleam_erlang = "~> 0.25"
mist = "~> 2.0"

# New additions
pgo = "~> 0.14"              # PostgreSQL client
decode = "~> 0.10"           # JSON decoding
gleam_crypto = "~> 1.3"      # Encryption
gleam_otp = "~> 0.13"        # OTP behaviors for agents
simplifile = "~> 2.4"        # File operations (existing)
envoy = "~> 1.0"             # Environment variables (existing)
```

### 5.3 Frontend Dependencies

```json
// manager/frontend/package.json additions
{
  "dependencies": {
    "react-markdown": "^9.0.0",        // Markdown rendering in chats
    "remark-gfm": "^4.0.0",            // GitHub flavored markdown
    "react-syntax-highlighter": "^15.5.0",  // Code highlighting
    "date-fns": "^3.0.0",              // Date formatting
    "react-beautiful-dnd": "^13.1.1"   // Drag-drop for kanban
  }
}
```

### 5.4 Message Queue Consideration

**For v1.0:** Use Postgres LISTEN/NOTIFY for task events
- Simple, no additional infrastructure
- Good for moderate concurrency

**For v2.0+:** Consider RabbitMQ or Redis Streams
- Better scalability
- Parallel task execution
- Multiple worker nodes

---

## 6. Implementation Phases & Priorities

### Phase 1: Foundation (Week 1-2)
**Priority: Critical**

1. Database setup
   - Create migrations directory
   - Write initial schema migration
   - Implement migration runner in Gleam
   - Add pgvector extension

2. Database layer
   - Postgres connection pool
   - Query modules for each entity
   - CRUD operations
   - Transaction support

3. API structure
   - Route organization
   - Request/response types
   - Error handling patterns
   - Validation helpers

**Deliverables:**
- Working database with all tables
- Database connection from Gleam
- Basic CRUD for one entity (Projects)

### Phase 2: Chats (Week 3)
**Priority: High**

1. Backend
   - Chat CRUD endpoints
   - Message creation with LiteLLM integration
   - Streaming responses (SSE or chunked)
   - Auto-title generation

2. Frontend
   - ChatsPage with list/view split
   - Message composer with model selection
   - Markdown rendering
   - Real-time message updates

**Deliverables:**
- Fully functional chat system
- Model selection working
- Conversation history persisted

### Phase 3: Projects & Tasks CRUD (Week 4)
**Priority: High**

1. Backend
   - Project CRUD endpoints
   - Task CRUD endpoints
   - Dependency validation
   - Status management

2. Frontend
   - ProjectsPage with card grid
   - Create/edit project modals
   - TasksPage with kanban board
   - Create/edit task modals
   - Task filtering & sorting

**Deliverables:**
- Project management UI
- Task management UI (no execution yet)
- Status workflows enforced

### Phase 4: Agent Executor (Week 5-6)
**Priority: Critical**

1. Agent system prompts
   - Architect planning prompt
   - Developer prompts (tests, impl, fixes)
   - Griller review prompt
   - Context building from project/task

2. Execution engine
   - Task run orchestrator
   - Phase execution sequence
   - Progress tracking
   - Error handling & retries
   - Artifact storage

3. WebSocket streaming
   - Real-time progress updates
   - Log streaming
   - Client reconnection handling

**Deliverables:**
- Working task execution
- Multi-agent workflow operational
- Real-time progress in UI

### Phase 5: GitHub Integration (Week 7)
**Priority: Medium**

1. Backend
   - GitHub API client
   - Repository clone/read/write
   - Commit creation
   - Pull request creation
   - Token encryption

2. Frontend
   - GitHub linking UI
   - Repository browser
   - Commit history view

**Deliverables:**
- Projects can link GitHub repos
- Agents can read/write code
- Commits created by agents

### Phase 6: Wiki Knowledge Base (Week 8-9)
**Priority: Medium**

1. Backend
   - Embedding generation (using Ollama embed model)
   - Content chunking
   - Vector search
   - Chat extraction (summarization)
   - URL ingestion (fetch + parse)

2. Frontend
   - Search interface
   - Manual ingestion form
   - Entry browsing
   - Chunk viewer

**Deliverables:**
- Working wiki search
- Manual content ingestion
- Automatic chat extraction (background job)

### Phase 7: Polish & Testing (Week 10)
**Priority: High**

1. Testing
   - Integration tests for APIs
   - E2E tests for critical flows
   - Load testing for task execution
   - WebSocket stability testing

2. UI/UX improvements
   - Loading states
   - Error messages
   - Responsive design
   - Accessibility

3. Documentation
   - API documentation
   - User guide
   - Architecture diagrams
   - Deployment guide

**Deliverables:**
- Production-ready system
- Test coverage >70%
- Complete documentation

---

## 7. Component Dependencies

### Critical Path
```
Database Schema (Phase 1)
    ↓
Chats Implementation (Phase 2)
    ↓
Projects & Tasks CRUD (Phase 3)
    ↓
Agent Executor (Phase 4)
    ↓
Full Task Execution
```

### Independent Tracks
- **GitHub Integration** can be developed in parallel with Phase 4
- **Wiki** can be developed in parallel with Phases 5-6
- **Frontend pages** can be scaffolded early with mock data

### Blockers
- Agent Executor blocks: Task execution, auto-testing, GitHub commits
- Database Schema blocks: All features
- LiteLLM integration blocks: Chats, Agent execution

---

## 8. Open Questions & Design Decisions

### 8.1 Model Selection Strategy
**Question:** Should tasks auto-select models based on capability, or require explicit configuration?

**Options:**
1. User selects per task (explicit control)
2. System selects based on task type (architect = reasoning model, developer = fast model)
3. Hybrid: default smart selection with override option

**Recommendation:** Option 3 - smart defaults with overrides

### 8.2 Task Artifact Storage
**Question:** Where to store agent outputs (code, plans, reviews)?

**Options:**
1. Database JSONB field (simple, limited size)
2. File system with references (scalable, complex)
3. S3-compatible storage (cloud-ready, requires setup)

**Recommendation:** Option 2 for v1.0 (local FS), migrate to Option 3 for production

### 8.3 Concurrent Task Execution
**Question:** Should multiple tasks run in parallel?

**Options:**
1. Sequential only (simple, no conflicts)
2. Parallel with locking (complex, faster)
3. Parallel with workspace isolation (best, complex)

**Recommendation:** Option 1 for v1.0, Option 3 for v2.0

### 8.4 Chat Knowledge Extraction
**Question:** When should chat content be extracted to Wiki?

**Options:**
1. Real-time during conversation (immediate, expensive)
2. Background job after chat closes (delayed, efficient)
3. Manual trigger only (user control, may be forgotten)

**Recommendation:** Option 2 with Option 3 as fallback

### 8.5 Authentication & Multi-User Support
**Question:** Should the system support multiple users?

**Current State:** Single API key for entire manager

**Options:**
1. Keep single-user (simpler, current model)
2. Add user accounts (complex, enables collaboration)

**Recommendation:** Option 1 for v1.0, design database schema to allow Option 2 migration

---

## 9. Performance Considerations

### 9.1 Database Optimization
- Index all foreign keys
- Index status/filter columns
- Use connection pooling (10-20 connections)
- EXPLAIN ANALYZE slow queries
- Consider materialized views for complex aggregations

### 9.2 WebSocket Scaling
- Use connection pooling
- Implement heartbeat/ping-pong
- Handle reconnection gracefully
- Consider Redis pub/sub for multi-instance deployment

### 9.3 Agent Execution
- Set timeouts for LLM calls (2-5 minutes)
- Implement retry logic with exponential backoff
- Stream large responses
- Cache common prompts
- Monitor token usage

### 9.4 Wiki Search
- Limit search to top-k chunks (k=10-20)
- Pre-filter by metadata before vector search
- Use IVFFlat index for large datasets (>10k chunks)
- Consider HNSW index for better recall

---

## 10. Security Considerations

### 10.1 GitHub Token Storage
- Encrypt at rest using `gleam_crypto`
- Never log tokens
- Rotate tokens periodically
- Use GitHub App installation tokens (auto-expiring)

### 10.2 API Authentication
- Current: Single API key via env var
- Future: Per-user tokens with scopes
- Rate limiting per client
- Request logging for audit

### 10.3 Code Execution Safety
- Agents should not execute arbitrary code
- Sandbox file system operations
- Validate file paths (prevent traversal)
- Limit repository clone size

### 10.4 Input Validation
- Validate all user inputs
- Sanitize markdown content
- Check URL schemes (allow http/https only)
- Limit request payload sizes

---

## 11. Monitoring & Observability

### 11.1 Metrics to Track
- Task execution times by phase
- Task success/failure rates
- LLM token usage & costs
- WebSocket connection counts
- Database query performance
- API endpoint latencies

### 11.2 Logging Strategy
- Structured logging (JSON)
- Log levels: DEBUG, INFO, WARNING, ERROR
- Context: request_id, user_id, task_id
- Rotate logs daily
- Ship to Loki or CloudWatch

### 11.3 Health Checks
- Database connectivity
- LiteLLM availability
- Ollama connectivity
- Disk space for repos/artifacts
- Memory usage

---

## 12. Future Enhancements (v2.0+)

### 12.1 Parallel Task Execution
- Execute independent tasks concurrently
- Workspace isolation per task
- Resource quotas (CPU, memory, tokens)

### 12.2 Custom Agent Workflows
- Define project-specific agent sequences
- Plugin system for custom agents
- Conditional execution (if tests pass, then deploy)

### 12.3 External Integrations
- Jira/Linear sync for tasks
- Slack notifications
- Email alerts
- Calendar integration for scheduled tasks

### 12.4 Team Collaboration
- Multi-user workspaces
- Role-based access control
- Comments on tasks
- @mentions in chat

### 12.5 Model Fine-tuning
- Collect project-specific data from Wiki
- Fine-tune models on codebase patterns
- Deploy custom models via Ollama

### 12.6 CI/CD Integration
- Webhook triggers from GitHub
- Automated testing on PR creation
- Deploy on task completion
- Rollback on failure

---

## 13. Success Metrics

### 13.1 Technical Metrics
- [ ] <500ms API response time (p95)
- [ ] >99% task execution success rate
- [ ] <5s WebSocket message latency
- [ ] >95% test coverage on core modules

### 13.2 User Experience Metrics
- [ ] Users can create and execute tasks end-to-end
- [ ] Chat responses feel natural and helpful
- [ ] Wiki search returns relevant results
- [ ] Real-time progress updates work reliably

### 13.3 Business Metrics
- [ ] Tasks complete without human intervention 80% of time
- [ ] Average task completion time <30 minutes
- [ ] Knowledge base grows organically from usage
- [ ] GitHub integration reduces manual coding time

---

## 14. Rollout Plan

### 14.1 Development Environment
- Use `docker-compose.dev.yml` for local development
- Hot reload for frontend (React dev server)
- Manual backend restart (or use watchexec)
- Seed database with sample data

### 14.2 Testing Environment
- Full docker-compose stack
- Test data fixtures
- Automated E2E tests
- Performance benchmarking

### 14.3 Production Deployment
- Environment variables for secrets
- Postgres backups enabled
- Volume mounts for persistence
- Health check monitoring
- Graceful shutdown handling

---

## Appendix A: Example API Payloads

### Create Chat
```json
POST /api/chats
{
  "model_name": "llama3.2",
  "first_message": "Hello, how can you help me today?"
}

Response 201:
{
  "id": "uuid",
  "title": "Conversation about help",
  "model_name": "llama3.2",
  "created_at": "2025-01-15T10:30:00Z",
  "messages": [
    {
      "id": "uuid",
      "role": "user",
      "content": "Hello, how can you help me today?",
      "created_at": "2025-01-15T10:30:00Z"
    },
    {
      "id": "uuid",
      "role": "assistant",
      "content": "I can assist with...",
      "created_at": "2025-01-15T10:30:05Z"
    }
  ]
}
```

### Create Task
```json
POST /api/tasks
{
  "project_id": "uuid",
  "title": "Implement user authentication",
  "description": "Add JWT-based auth to the API",
  "acceptance_criteria": "- Users can login\n- Tokens expire after 24h\n- Protected routes return 401",
  "priority": 1,
  "model_name": "deepseek-coder:33b",
  "dependencies": []
}

Response 201:
{
  "id": "uuid",
  "project_id": "uuid",
  "title": "Implement user authentication",
  "status": "created",
  "priority": 1,
  "created_at": "2025-01-15T10:30:00Z"
}
```

### Start Task Execution
```json
POST /api/tasks/{id}/start

Response 200:
{
  "run_id": "uuid",
  "status": "running",
  "current_phase": "architect",
  "progress_percent": 0,
  "started_at": "2025-01-15T10:30:00Z"
}
```

### WebSocket Progress Update
```json
{
  "type": "progress",
  "run_id": "uuid",
  "phase": "developer_impl",
  "progress_percent": 65,
  "message": "Implementing authentication middleware"
}
```

---

## Appendix B: Database Migration Example

```sql
-- migrations/001_initial_schema.sql
BEGIN;

-- Chats
CREATE TABLE chats (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  title TEXT NOT NULL,
  model_name TEXT NOT NULL,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  archived BOOLEAN DEFAULT FALSE
);

CREATE TABLE messages (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  chat_id UUID NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
  role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system')),
  content TEXT NOT NULL,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  metadata JSONB DEFAULT '{}'::jsonb
);

CREATE INDEX idx_messages_chat_id ON messages(chat_id);
CREATE INDEX idx_messages_created_at ON messages(created_at);

-- Projects
CREATE TABLE projects (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name TEXT NOT NULL,
  description TEXT,
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'on_hold', 'cancelled')),
  github_repo_url TEXT,
  github_access_token TEXT,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE INDEX idx_projects_status ON projects(status);

-- Tasks
CREATE TABLE tasks (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  description TEXT NOT NULL,
  acceptance_criteria TEXT,
  status TEXT NOT NULL DEFAULT 'created' CHECK (status IN
    ('created', 'queued', 'in_progress', 'review', 'complete', 'blocked')),
  priority INTEGER DEFAULT 3 CHECK (priority >= 1 AND priority <= 5),
  model_name TEXT,
  dependencies JSONB DEFAULT '[]'::jsonb,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  started_at TIMESTAMP WITH TIME ZONE,
  completed_at TIMESTAMP WITH TIME ZONE
);

CREATE INDEX idx_tasks_project_id ON tasks(project_id);
CREATE INDEX idx_tasks_status ON tasks(status);

-- Task Runs
CREATE TABLE task_runs (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  status TEXT NOT NULL CHECK (status IN ('running', 'completed', 'failed', 'cancelled')),
  current_phase TEXT,
  progress_percent INTEGER DEFAULT 0 CHECK (progress_percent >= 0 AND progress_percent <= 100),
  started_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  completed_at TIMESTAMP WITH TIME ZONE,
  error_message TEXT,
  artifacts JSONB DEFAULT '{}'::jsonb
);

CREATE INDEX idx_task_runs_task_id ON task_runs(task_id);
CREATE INDEX idx_task_runs_status ON task_runs(status);

-- Task Run Logs
CREATE TABLE task_run_logs (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  task_run_id UUID NOT NULL REFERENCES task_runs(id) ON DELETE CASCADE,
  phase TEXT NOT NULL,
  agent_type TEXT NOT NULL,
  log_level TEXT NOT NULL CHECK (log_level IN ('debug', 'info', 'warning', 'error')),
  message TEXT NOT NULL,
  metadata JSONB DEFAULT '{}'::jsonb,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE INDEX idx_task_run_logs_run_id ON task_run_logs(task_run_id);
CREATE INDEX idx_task_run_logs_created_at ON task_run_logs(created_at);

-- Migration tracking
CREATE TABLE schema_migrations (
  version INTEGER PRIMARY KEY,
  applied_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

INSERT INTO schema_migrations (version) VALUES (1);

COMMIT;
```

---

## Appendix C: Agent Prompt Templates

### Architect Planning Prompt
```
You are an expert software architect planning the implementation of a task.

PROJECT: {project_name}
{project_description}

TASK: {task_title}
{task_description}

ACCEPTANCE CRITERIA:
{acceptance_criteria}

Your goal is to:
1. Analyze the requirements thoroughly
2. Design a high-level implementation approach
3. Break down the work into logical steps
4. Identify potential risks or challenges
5. Define success criteria

Provide a structured implementation plan in markdown format.
Include file structure, key functions/classes, and testing strategy.
```

### Developer Test Prompt
```
You are a test-driven developer writing tests for a new feature.

IMPLEMENTATION PLAN:
{architect_plan}

TASK: {task_title}
{task_description}

Your goal is to:
1. Write comprehensive test cases that validate the requirements
2. Cover happy paths, edge cases, and error conditions
3. Use appropriate testing frameworks for the language/stack
4. Ensure tests are runnable and well-documented

Provide test files with clear test names and assertions.
```

### Griller Review Prompt
```
You are a thorough code reviewer (The Griller) examining an implementation.

ORIGINAL PLAN:
{architect_plan}

IMPLEMENTATION:
{developer_code}

Your goal is to:
1. Verify the implementation matches the plan
2. Check for bugs, security issues, and performance problems
3. Evaluate code quality, readability, and maintainability
4. Suggest specific improvements with examples
5. Flag any deviations from best practices

Provide detailed feedback in markdown format with severity levels (Critical/Major/Minor).
Be constructive but thorough - your review ensures quality.
```

---

## Summary

This implementation plan provides a complete roadmap for building Zone Manager from the current state (working Models page) to a fully functional AI orchestration platform. The plan prioritizes foundational infrastructure first, then builds features incrementally with clear dependencies and deliverables.

**Key Success Factors:**
1. **Database-first approach** - All features depend on solid data layer
2. **Incremental delivery** - Each phase delivers working functionality
3. **Real-time feedback** - WebSocket progress keeps users informed
4. **Extensible design** - System ready for v2.0 enhancements
5. **Security-conscious** - Auth, encryption, and validation throughout

**Estimated Timeline:** 10 weeks for v1.0 MVP
**Team Size:** 1-2 developers (can be split into frontend/backend tracks)
**Risk Level:** Medium (new agent coordination complexity, GitHub integration unknowns)

The architecture leverages existing infrastructure (Postgres, LiteLLM, Ollama) while adding minimal new dependencies, keeping operational complexity manageable.
