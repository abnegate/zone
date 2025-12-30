# Zone Manager Vision

## Overview

Zone Manager is an AI orchestration platform that enables autonomous software development through coordinated multi-agent workflows. The system manages local AI models and provides a framework for projects, tasks, conversations, and a growing knowledge base.

## Core Concepts

### Chats (Conversations)

Chats are direct conversations with AI models. They serve multiple purposes:

- Interactive problem-solving and ideation
- Model testing and experimentation
- Knowledge capture - all conversations automatically contribute to the Wiki knowledge base

Conversations can select from any installed model and maintain full context throughout the session.

### Projects

Projects are organizational containers that group related work together. Key characteristics:

- **Status Tracking**: Active, On Hold, or Cancelled
- **GitHub Integration**: Projects can optionally link to a GitHub repository, enabling agents to read/write code directly
- **Task Container**: Projects contain multiple tasks that define the work to be done
- **Independence**: Projects do not require a repository - they can represent any body of work

Projects represent ongoing initiatives, product features, research efforts, or any logical grouping of tasks.

### Tasks

Tasks are the atomic units of work within projects. They define what needs to be accomplished and are executed by autonomous agent workflows.

#### Task Lifecycle

1. **Created**: Task defined with requirements and acceptance criteria
2. **Queued**: Waiting for agent assignment
3. **In Progress**: Agent workflow actively working on the task
4. **Review**: Awaiting final approval
5. **Complete**: Task finished and verified
6. **Blocked**: Cannot proceed due to external dependency

#### Agent Workflow

When a task is started, a coordinated multi-agent workflow executes asynchronously:

```
┌─────────────────────────────────────────────────────────────────┐
│                        TASK EXECUTION                          │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  1. ARCHITECT                                                   │
│     └─→ Analyzes requirements                                   │
│     └─→ Plans implementation approach                           │
│     └─→ Defines subtasks and dependencies                       │
│                                                                 │
│  2. DEVELOPER (Phase 1: Testing)                                │
│     └─→ Writes test cases based on requirements                 │
│     └─→ Establishes test coverage expectations                  │
│                                                                 │
│  3. DEVELOPER (Phase 2: Implementation)                         │
│     └─→ Implements the feature/fix                              │
│     └─→ Ensures tests pass                                      │
│                                                                 │
│  4. GRILLER (Code Review)                                       │
│     └─→ Reviews implementation                                  │
│     └─→ Identifies issues, improvements, edge cases             │
│     └─→ Provides detailed feedback                              │
│                                                                 │
│  5. DEVELOPER (Phase 3: Fixes)                                  │
│     └─→ Addresses Griller feedback                              │
│     └─→ Refines implementation                                  │
│                                                                 │
│  6. ARCHITECT (Final Review)                                    │
│     └─→ Validates against original plan                         │
│     └─→ Ensures architectural integrity                         │
│     └─→ Approves or requests changes                            │
│                                                                 │
│  7. DEVELOPER (Phase 4: Final Fixes)                            │
│     └─→ Addresses Architect feedback                            │
│     └─→ Prepares final deliverable                              │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

#### Task Assignment

Tasks can be assigned:

- A specific model to use for execution
- Priority level
- Dependencies on other tasks
- Resource constraints

#### Async Progress API

The task execution is fully asynchronous. An API provides real-time updates:

- Current agent/phase
- Progress percentage
- Log output
- Artifacts produced
- Blockers encountered

Clients can poll or subscribe to WebSocket updates for live progress tracking.

### Models

The Models section manages local AI models through Ollama:

- Browse available models from the Ollama library
- Pull and install models locally
- Monitor model status and resource usage
- Configure model parameters
- Remove unused models

### Wiki (Knowledge Base)

The Wiki is an evolving knowledge repository that grows organically and through intentional curation.

#### Knowledge Sources

1. **Automatic Extraction**: Conversations in Chats are analyzed for useful information. Key facts, decisions, code patterns, and learnings are extracted and indexed.

2. **Intentional Content**: Users can directly feed content to the Wiki:
   - Documentation URLs
   - Reference links
   - Text documents
   - Code repositories
   - API specifications

3. **Task Outputs**: Completed tasks contribute learnings back to the Wiki - architectural decisions, implementation patterns, and lessons learned.

#### Knowledge Utilization

Models can query the Wiki during conversations and task execution to:

- Retrieve project-specific context
- Access documented patterns and conventions
- Reference previous decisions
- Find relevant code examples

The Wiki enables models to maintain institutional knowledge and improve responses over time.

## Architecture

### Component Overview

```
┌──────────────────────────────────────────────────────────────┐
│                     ZONE MANAGER                             │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │   Frontend  │  │   Backend   │  │   Agent Executor    │  │
│  │   (React)   │──│   (Gleam)   │──│   (Multi-Agent)     │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
│         │               │                    │               │
│         └───────────────┼────────────────────┘               │
│                         │                                    │
│                         ▼                                    │
│  ┌─────────────────────────────────────────────────────┐    │
│  │                    Data Layer                        │    │
│  ├─────────────┬─────────────┬─────────────────────────┤    │
│  │   Projects  │    Tasks    │     Wiki Index          │    │
│  │   & Chats   │   & Runs    │   & Embeddings          │    │
│  └─────────────┴─────────────┴─────────────────────────┘    │
│                                                              │
└──────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────┐
│                      OLLAMA / LiteLLM                        │
│                    (Model Execution)                         │
└──────────────────────────────────────────────────────────────┘
```

### Agent Executor

The Agent Executor is responsible for running task workflows. It:

- Spawns sub-agents (Architect, Developer, Griller) as needed
- Manages handoffs between agents
- Tracks progress and state
- Handles failures and retries
- Reports status via API

### API Endpoints

Key API capabilities:

- `POST /tasks/{id}/start` - Begin task execution
- `POST /tasks/{id}/stop` - Halt running task
- `GET /tasks/{id}/progress` - Get current progress
- `WS /tasks/{id}/stream` - Real-time progress stream
- `POST /wiki/ingest` - Add content to Wiki
- `GET /wiki/search` - Query knowledge base

## Future Directions

### Planned Enhancements

- **Parallel Task Execution**: Run independent tasks concurrently
- **Custom Agent Workflows**: Define project-specific agent sequences
- **External Integrations**: Jira, Linear, Notion sync
- **Team Collaboration**: Multi-user workspaces
- **Model Fine-tuning**: Train models on project-specific data from Wiki
- **Automated Testing**: Integration with CI/CD pipelines

### Extensibility

The platform is designed to be extensible:

- Plugin system for custom agents
- Webhook integrations for external events
- API-first design for third-party tools
- Custom workflow definitions

## Guiding Principles

1. **Autonomous by Default**: Tasks should complete without human intervention when possible
2. **Transparent Progress**: Always show what agents are doing and why
3. **Knowledge Accumulation**: Every interaction should potentially improve the system
4. **Quality Gates**: Multiple review stages ensure high-quality output
5. **Human Override**: Users can intervene at any point in the workflow
6. **Local First**: Models run locally for privacy and control
