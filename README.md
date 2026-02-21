# CalDAV/ICS Sync

A self-hosted bidirectional synchronization service between CalDAV servers and ICS files. Manage multiple sync configurations through a web UI or REST API.

Built with Rust (Axum) backend and Next.js frontend. All configuration and data stored in SQLite.

## Interface

![UI](assets/UI.png)

## Features

- **CalDAV to ICS (Sources)** -- Pull events from CalDAV servers and serve them as subscribable ICS endpoints
- **ICS to CalDAV (Destinations)** -- Push events from ICS files to CalDAV servers with configurable sync behavior
- **Multi-source/destination management** -- Add, edit, and delete configurations via the web UI or API
- **Custom ICS paths** -- Each source gets a user-defined URL path (e.g., `/ics/work-calendar`)
- **Automatic background sync** -- Per-source/destination configurable sync intervals
- **Sync options** -- Control whether to sync past events (`sync_all`) and whether to preserve local CalDAV events not in ICS (`keep_local`)
- **Trailing slash compatibility** -- Automatically retries CalDAV requests with toggled trailing slash for servers like Feishu/Nextcloud
- **Password security** -- Passwords are never returned in API responses; stored in plain text for CalDAV authentication
- **OpenAPI spec** -- Full API documentation at `/api/openapi.json`
- **Health checks** -- `/api/health` and `/api/health/detailed` endpoints with live status in the UI
- **Windows Fluent UI** -- Dashboard styled with windows-ui-fabric for a native Windows look

## Quick Start (Docker)

```bash
docker run -d \
  --name cal-sync \
  -p 6765:6765 \
  -v $(pwd)/data:/data \
  ghcr.io/robbyv2/caldav-ics-sync:latest
```

Open `http://localhost:6765` to access the dashboard.

## Docker Compose

### Basic

```yaml
services:
  cal-sync:
    image: ghcr.io/robbyv2/caldav-ics-sync:latest
    container_name: cal-sync
    ports:
      - "6765:6765"
    volumes:
      - ./data:/data
    restart: unless-stopped
```

### With HTTP Basic Auth

Since the app has no built-in authentication, you can front it with an nginx basic auth proxy:

```yaml
services:
  cal-sync:
    image: ghcr.io/robbyv2/caldav-ics-sync:latest
    container_name: cal-sync
    volumes:
      - ./data:/data
    restart: unless-stopped

  proxy:
    image: beevelop/nginx-basic-auth
    container_name: cal-sync-proxy
    ports:
      - "6765:80"
    environment:
      - FORWARD_HOST=cal-sync
      - FORWARD_PORT=6765
      - HTPASSWD=admin:$$apr1$$odHl5EJN$$KbxMfo86Qdve2FH4owePn.
    depends_on:
      - cal-sync
    restart: unless-stopped
```

> [!NOTE]
> Generate your own credentials with `htpasswd -nb admin yourpassword` and replace the `HTPASSWD` value. Use `$$` to escape `$` signs in docker compose.

## Configuration

All sync configuration (sources, destinations, credentials) is managed through the web UI. The only environment variables are for server tuning:

| Variable           | Default                 | Description                    |
| ------------------ | ----------------------- | ------------------------------ |
| `SERVER_HOST`      | `0.0.0.0`               | Bind address                   |
| `SERVER_PORT`      | `6765`                  | Rust server port (user-facing) |
| `PORT`             | `6766`                  | Next.js internal port          |
| `SERVER_PROXY_URL` | `http://localhost:6766` | Internal proxy target          |
| `DATA_DIR`         | `./data`                | Directory for SQLite database  |

## Concepts

### Sources (CalDAV to ICS)

A source pulls events from a CalDAV server and exposes them as an ICS file at a custom path. Configure:

- CalDAV URL, username, and password
- ICS path (the URL path where the ICS file is served, e.g., `/ics/my-calendar`)
- Sync interval (seconds/minutes/hours, 0 for manual only)

### Destinations (ICS to CalDAV)

A destination downloads an ICS file from a URL and uploads each event to a CalDAV server. Inspired by [ics_caldav_sync](https://github.com/przemub/ics_caldav_sync). Configure:

- ICS source URL (the remote ICS file to download)
- CalDAV server URL, calendar name, username, and password
- Sync interval (seconds/minutes/hours)
- `sync_all` -- whether to sync past events or only future ones
- `keep_local` -- whether to preserve CalDAV events that don't exist in the ICS file

## API

The full OpenAPI spec is available at `/api/openapi.json`.

### Sources

| Method   | Path                      | Description      |
| -------- | ------------------------- | ---------------- |
| `GET`    | `/api/sources`            | List all sources |
| `POST`   | `/api/sources`            | Create a source  |
| `PUT`    | `/api/sources/:id`        | Update a source  |
| `DELETE` | `/api/sources/:id`        | Delete a source  |
| `POST`   | `/api/sources/:id/sync`   | Trigger sync     |
| `GET`    | `/api/sources/:id/status` | Source status    |
| `GET`    | `/ics/:path`              | Serve ICS file   |

### Destinations

| Method   | Path                         | Description           |
| -------- | ---------------------------- | --------------------- |
| `GET`    | `/api/destinations`          | List all destinations |
| `POST`   | `/api/destinations`          | Create a destination  |
| `PUT`    | `/api/destinations/:id`      | Update a destination  |
| `DELETE` | `/api/destinations/:id`      | Delete a destination  |
| `POST`   | `/api/destinations/:id/sync` | Trigger reverse sync  |

### Health

| Method | Path                   | Description     |
| ------ | ---------------------- | --------------- |
| `GET`  | `/api/health`          | Health check    |
| `GET`  | `/api/health/detailed` | Detailed health |

## Local Development

All commands use [just](https://github.com/casey/just) via the `jfiles/` directory.

```bash
just src install    # Install dependencies
just src dev        # Run both servers (Rust + Next.js) with hot reload
just src fmt        # Format and lint all code
just src build-all  # Full production build
just src prod       # Build and run production
```

Navigate to `http://127.0.0.1:6765`.

## Data Storage

All configuration and synced ICS data is stored in a single SQLite database at `DATA_DIR/caldav-sync.db`. Mount `/data` as a Docker volume for persistence.
