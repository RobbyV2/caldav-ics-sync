# CalDAV to ICS Synchronizer

A standalone server to synchronize CalDAV events from various enterprise and personal servers (such as Feishu, iCloud, Nextcloud) to a unified ICS file format.

This project utilizes a Rust/Axum API backend with XML parsing capabilities alongside a Next.js 15 frontend.

## Key Features

- **Automated Background Synchronization:** Set `AUTO_SYNC_INTERVAL_MINUTES` in your `.env` to synchronize calendars in the background.
- **Feishu / Strict CalDAV Compatibility:** Bypasses parser bugs (e.g., `python-caldav` Issue #459) by interacting directly at the raw WebDAV and `PROPFIND` layers, extracting `VEVENT` data strings without strict payload evaluation.
- **Flexible Storage Policies:** Save synced calendars to the disk for persistence or keep them in volatile memory.
- **Tech Stack:**
  - Rust Backend (Axum, Tokio, Reqwest, Roxmltree) handles requests over port 3000.
  - Next.js (React 19) provides the synchronization interface proxying requests from port 3001.

---

## Running the Project

### Environment Initialization

Copy the standard template for environment details.

```bash
cp .env.example .env.local
```

Fill in your CalDAV properties within `.env.local`:

```env
# CalDAV Authentication & URL
CALDAV_URL=https://your-caldav-server.example.com
CALDAV_USERNAME=username
CALDAV_PASSWORD=secret_password

# ICS Storage Configuration
# Strategies: 'memory-only', 'disk-only', 'memory-and-disk'
STORAGE_STRATEGY=memory-and-disk
STORAGE_DISK_PATH=./data/caldav-sync-cache.ics

# Background sync interval in minutes
AUTO_SYNC_INTERVAL_MINUTES=60
```

### Docker (Recommended)

This service is fully containerized with a multi-stage build and published to `ghcr.io`.

```bash
docker run -d \
  --name caldav-sync \
  -p 6765:3000 \
  -e SERVER_HOST=0.0.0.0 \
  -e SERVER_PORT=3000 \
  -e CALDAV_URL=https://your-caldav-server.example.com \
  -e CALDAV_USERNAME=username \
  -e CALDAV_PASSWORD=secret_password \
  -e STORAGE_STRATEGY=memory-and-disk \
  -e STORAGE_DISK_PATH=/data/caldav-sync-cache.ics \
  -e AUTO_SYNC_INTERVAL_MINUTES=60 \
  -v $(pwd)/data:/data \
  ghcr.io/robbyv2/caldav-to-ics:latest
```

### Local Development

To work on the toolset locally:

1. **Install Frontend Dependencies:**

```bash
bun install --frozen-lockfile
```

1. **Launch development servers:**

```bash
# Terminal 1 - Next.js
bun run dev

# Terminal 2 - Rust Backend
cargo run
```

Next.js acts behind port `3001`, navigate to the Rust Gateway available at `http://127.0.0.1:3000`.

---

## User Interface

By visiting the entry port, you can view the status interface to check background completion times and trigger manual synchronization.

## Open Source Details

- **License**: MIT
- **CI/CD Configuration**: `.github/workflows/ci.yml` tests linting layouts using TS-ESLint Flat architecture and publishes new semantic versions pushed via tags to `ghcr.io`.
