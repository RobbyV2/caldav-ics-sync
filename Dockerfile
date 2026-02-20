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

EXPOSE 3000
EXPOSE 3001

ENV SERVER_PORT=3000
ENV PORT=3001
ENV SERVER_PROXY_URL=http://localhost:3001

RUN echo '#!/bin/sh\nbun server.js &\n./server\n' > /app/start.sh && \
    chmod +x /app/start.sh

VOLUME ["/data"]

CMD ["/app/start.sh"]
