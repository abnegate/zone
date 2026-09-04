# Zone Web Installer

A web-based configuration wizard for the Zone AI Stack.

## Technology Stack

- **Backend**: Rust (Axum web framework)
- **Frontend**: React with TypeScript
- **CSS**: Tailwind CSS

## Features

- Configuration wizard for Zone chat and services
- One-click secret generation
- Inline form validation
- Live installation progress
- Mobile-responsive UI
- OpenVPN & WireGuard support
- Completion link to Zone chat at `/chats`

## Development

### Prerequisites
- Rust 1.97.1+
- Bun 1.0+

### Build Locally

```bash
# Build the Rust backend
cd runner && cargo build --release --package zone_installer

# Build the frontend
cd installer/frontend && bun install && bun run build
```

### Build Docker Image

```bash
# Build from repo root
docker build -f installer/Dockerfile .
docker run -p 8000:8000 -v $PWD:/project zone-installer
```

### Using with Make

```bash
make install  # From project root
```

## API Endpoints

### `GET /`
Serves the installer UI (React SPA)

### `GET /api/health`
Health check endpoint

### `POST /api/install`
Handles installation request

**Request Body:**
```json
{
  "DOMAIN_HOST_WEBUI": "webui.localhost",
  "SECURITY_LITELLM_MASTER_KEY": "...",
  "OLLAMA_MODEL_FAST": "llama3.1:8b",
  ...
}
```

**Response:** Streaming JSON progress updates
```json
{"progress": 10, "status": "Configuration received"}
{"progress": 40, "status": ".env file created"}
...
{"progress": 100, "complete": true}
```

## Project Structure

```
installer/
├── frontend/              # React frontend
│   ├── src/
│   │   ├── components/    # UI components
│   │   ├── pages/         # Page components
│   │   └── steps/         # Wizard step components
│   ├── public/            # Static assets
│   └── package.json       # Frontend dependencies
├── Dockerfile             # Multi-stage build
└── README.md              # This file

runner/
└── zone_installer/        # Rust backend
    └── src/
        └── main.rs        # Installer server
```

## License

MIT - Same as parent project
