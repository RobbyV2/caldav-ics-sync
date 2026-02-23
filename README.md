# CalDAV/ICS Sync

<div align="center">
  <p>
    <img src="https://github.com/RobbyV2/caldav-ics-sync/actions/workflows/ci.yml/badge.svg" alt="CI">
    <img src="https://img.shields.io/github/v/release/RobbyV2/caldav-ics-sync?color=green&label=Version" alt="Version">
    <img src="https://img.shields.io/badge/License-MIT-green.svg" alt="License">
  </p>
</div>

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
- **Password security** -- Passwords are never returned in API responses; stored in plain text for CalDAV authentication. Sending an empty password on update preserves the existing value
- **OpenAPI spec** -- Full API documentation at `/api/openapi.json`
- **Health checks** -- `/api/health` and `/api/health/detailed` endpoints with live status in the UI
- **Public ICS URLs** - Optionally expose ICS feeds without authentication for Google Calendar and similar services
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

```yaml
services:
  cal-sync:
    image: ghcr.io/robbyv2/caldav-ics-sync:latest
    container_name: cal-sync
    ports:
      - '6765:6765'
    volumes:
      - ./data:/data
    # Optional: uncomment to enable HTTP Basic Auth on all routes.
    # Use AUTH_PASSWORD for plain text or AUTH_PASSWORD_HASH for argon2 (not both).
    # environment:
    #   - AUTH_USERNAME=admin
    #   - AUTH_PASSWORD=yourpassword
    #   # Or use a hashed password instead:
    #   # - AUTH_PASSWORD_HASH=$$argon2id$$v=19$$m=19456,t=2,p=1$$...
    restart: unless-stopped
```

> [!NOTE]
> `AUTH_USERNAME` plus exactly one of `AUTH_PASSWORD` or `AUTH_PASSWORD_HASH` enables auth. Setting both password vars is an error. Generate a hash with: `echo -n "yourpassword" | argon2 yoursalt -id -e`

> [!IMPORTANT]
> When setting `AUTH_PASSWORD_HASH` via docker compose environment variables, you _must_ escape each `$` with another `$` (or just pass in an env file)

## Configuration

All sync configuration (sources, destinations, credentials) is managed through the web UI. The only environment variables are for server tuning:

| Variable             | Default                   | Description                                            |
| -------------------- | ------------------------- | ------------------------------------------------------ |
| `SERVER_HOST`        | `0.0.0.0`                 | Bind address                                           |
| `SERVER_PORT`        | `6765`                    | Rust server port (user-facing)                         |
| `PORT`               | `6766`                    | Next.js internal port                                  |
| `SERVER_PROXY_URL`   | `http://localhost:6766`   | Internal proxy target                                  |
| `DATA_DIR`           | `./data`                  | Directory for SQLite database                          |
| `DB_PATH`            | `DATA_DIR/caldav-sync.db` | Full path to SQLite database file                      |
| `AUTH_USERNAME`      | _(unset)_                 | Basic Auth username (required to enable auth)          |
| `AUTH_PASSWORD`      | _(unset)_                 | Plain text password (mutually exclusive with hash)     |
| `AUTH_PASSWORD_HASH` | _(unset)_                 | Argon2 PHC-format hash (mutually exclusive with above) |

## Concepts

### Sources (CalDAV to ICS)

A source pulls events from a CalDAV server and exposes them as an ICS file at a custom path. Configure:

- CalDAV URL, username, and password
- ICS path (the URL path where the ICS file is served, e.g., `/ics/my-calendar`)
- Sync interval (seconds/minutes/hours, 0 for manual only)

#### Public ICS URLs

Sources can optionally make their ICS feed publicly accessible (without HTTP Basic Auth). Enable via the "Make ICS URL public" checkbox when creating or editing a source.

- **With custom path**: A dedicated URL at `/ics/public/{custom-path}` serves the feed without auth. The standard `/ics/{path}` still requires credentials.
- **Without custom path** (field left empty): The standard `/ics/{path}` URL becomes accessible without auth.

This is useful for services like Google Calendar that cannot supply HTTP Basic Auth credentials when subscribing to ICS feeds.

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

| Method   | Path                      | Description                              |
| -------- | ------------------------- | ---------------------------------------- |
| `GET`    | `/api/sources`            | List all sources                         |
| `POST`   | `/api/sources`            | Create a source                          |
| `PUT`    | `/api/sources/:id`        | Update a source                          |
| `DELETE` | `/api/sources/:id`        | Delete a source                          |
| `POST`   | `/api/sources/:id/sync`   | Trigger sync                             |
| `GET`    | `/api/sources/:id/status` | Source status                            |
| `GET`    | `/ics/:path`              | Serve ICS file                           |
| `GET`    | `/ics/public/:path`       | Serve public ICS feed (no auth required) |

### Source Paths

Additional ICS/public paths per source, managed via API (not shown in the UI).

| Method   | Path                              | Description             |
| -------- | --------------------------------- | ----------------------- |
| `GET`    | `/api/sources/:id/paths`          | List paths for a source |
| `POST`   | `/api/sources/:id/paths`          | Add a path to a source  |
| `PUT`    | `/api/sources/:id/paths/:path_id` | Update a source path    |
| `DELETE` | `/api/sources/:id/paths/:path_id` | Delete a source path    |

Each source path has a `path` (served at `/ics/{path}`) and an `is_public` flag. When `is_public` is true, the path is also accessible without authentication at `/ics/public/{path}`, and the standard `/ics/{path}` URL is auth-exempt. Paths are validated for uniqueness across all sources and source paths.

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

All configuration and synced ICS data is stored in a single SQLite database. By default this is at `DATA_DIR/caldav-sync.db`, but can be overridden with the `DB_PATH` environment variable. Mount `/data` as a Docker volume for persistence.
