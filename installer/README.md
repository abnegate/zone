# Voiz Web Installer

A web-based configuration wizard for the Voiz AI Stack.

## Technology Stack

- **Backend**: Gleam (functional language on the BEAM/Erlang VM)
- **Web Framework**: Wisp + Mist (Gleam HTTP server)
- **Frontend**: Vanilla HTML + JavaScript
- **CSS**: Tailwind CSS (CDN)

## Features

- 7-step configuration wizard
- One-click secret generation
- Inline form validation
- Live installation progress
- Mobile-responsive UI
- OpenVPN & WireGuard support
- Zero npm/build step required

## Why Gleam?

Gleam provides:
- ✅ Type safety
- ✅ Pattern matching
- ✅ Excellent concurrency (BEAM VM)
- ✅ Small binary size
- ✅ Fast compilation
- ✅ Functional programming paradigm
- ✅ Great developer experience

## Development

### Prerequisites
- Gleam 1.7.1+
- Erlang 27+

### Build Locally
```bash
cd installer
gleam build
gleam run
```

### Build Docker Image
```bash
docker build -t voiz-installer .
docker run -p 8000:8000 -v $PWD/..:/project voiz-installer
```

### Using with Make
```bash
make install  # From project root
```

## API Endpoints

### `GET /`
Serves the installer UI (HTML)

### `GET /static/{file}`
Serves static assets (JavaScript)

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
├── gleam.toml              # Gleam project configuration
├── src/
│   └── voiz_installer.gleam  # Main application
├── templates/
│   └── index.html          # Wizard UI
├── static/
│   └── installer.js        # Frontend logic
├── Dockerfile              # Multi-stage Gleam build
└── README.md               # This file
```

## Dependencies

Defined in `gleam.toml`:
- **gleam_stdlib** - Standard library
- **gleam_http** - HTTP primitives
- **gleam_json** - JSON encoding/decoding
- **wisp** - Web framework
- **mist** - HTTP server
- **simplifile** - File system operations

## License

MIT - Same as parent project
