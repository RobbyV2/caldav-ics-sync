# Stage 1: Build the Rust Backend
FROM rust:1.93-slim-bookworm AS rust-builder
WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

# Stage 2: Build the Next.js Frontend
FROM oven/bun:1.1 AS js-builder
WORKDIR /app

# Not a glob: `bun install --frozen-lockfile` silently re-resolves everything when the
# lockfile is absent, so a missing bun.lock must fail the build instead of quietly
# producing an image with unpinned dependencies.
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile

COPY . ./

RUN bun run build

# Stage 3: Final runtime image
FROM debian:bookworm-slim AS runner
WORKDIR /app

RUN apt-get update && apt-get install -y ca-certificates libssl3 curl && rm -rf /var/lib/apt/lists/*

# Copy bun from its official image rather than curl-piping the installer at build time:
# no network fetch of an unpinned script, and the version tracks the js-builder stage.
COPY --from=oven/bun:1.1 /usr/local/bin/bun /usr/local/bin/bun

ENV NODE_ENV=production
ENV NEXT_TELEMETRY_DISABLED=1

# Copy Next.js standalone build
COPY --from=js-builder /app/.next/standalone ./
COPY --from=js-builder /app/.next/static ./.next/static
COPY --from=js-builder /app/public ./public

# Copy Rust backend
COPY --from=rust-builder /app/target/release/server /app/server

EXPOSE 6765

ENV SERVER_HOST=0.0.0.0
ENV SERVER_PORT=6765
ENV PORT=6766
ENV HOSTNAME=0.0.0.0
ENV SERVER_PROXY_URL=http://localhost:6766
ENV DATA_DIR=/data

COPY <<'EOF' /app/start.sh
#!/bin/sh
set -e

bun server.js &
NEXT_PID=$!
trap "kill $NEXT_PID 2>/dev/null; exit" TERM INT
# Give Next.js a moment to bind before the Rust server starts proxying to it.
sleep 1
./server
kill $NEXT_PID 2>/dev/null
EOF
RUN chmod +x /app/start.sh

VOLUME ["/data"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:6765/api/health || exit 1

CMD ["/app/start.sh"]
