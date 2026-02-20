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

COPY package.json bun.lock* ./
RUN bun install --frozen-lockfile

COPY . ./

RUN bun run build

# Stage 3: Final runtime image
FROM debian:bookworm-slim AS runner
WORKDIR /app

RUN apt-get update && apt-get install -y ca-certificates libssl3 curl unzip && rm -rf /var/lib/apt/lists/*

# Install bun for running Next.js standalone server
RUN curl -fsSL https://bun.sh/install | bash
ENV PATH="/root/.bun/bin:${PATH}"

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
ENV SERVER_PROXY_URL=http://localhost:6766
ENV DATA_DIR=/data

RUN printf '#!/bin/sh\nbun server.js &\nNEXT_PID=$!\ntrap "kill $NEXT_PID 2>/dev/null; exit" SIGTERM SIGINT\n./server\nkill $NEXT_PID 2>/dev/null\n' > /app/start.sh && \
    chmod +x /app/start.sh

VOLUME ["/data"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:6765/api/health || exit 1

CMD ["/app/start.sh"]
