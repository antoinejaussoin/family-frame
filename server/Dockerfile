# Multi-stage: build the family SPA, then the Rust server.
#
# VERSION is passed as a build-arg (GitHub Actions / `make docker-build`) so
# the image, `eink-frame --version`, and GET /health all match the repo-root
# VERSION file without rewriting Cargo.toml.

FROM node:22-bookworm AS ui
WORKDIR /ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci
COPY ui/ ./
ARG VERSION=0.0.0
ENV FAMILY_FRAME_VERSION=$VERSION
RUN npm run build

FROM rust:1-bookworm AS builder

WORKDIR /src

# Compile crates.io dependencies in their own layer so source-only changes
# reuse the cached release artifacts.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
RUN mkdir src \
    && echo "fn main() {}" > src/main.rs \
    && echo "" > src/lib.rs \
    && cargo build --release --locked \
    && rm -rf src \
    && rm -f target/release/deps/eink_frame* target/release/libeink_frame* target/release/eink-frame

COPY src ./src
COPY templates ./templates
COPY static ./static
COPY build.rs ./
ARG VERSION=0.0.0
ENV FAMILY_FRAME_VERSION=$VERSION
RUN touch src/lib.rs src/main.rs \
    && cargo build --release --locked

FROM debian:bookworm-slim

ARG VERSION=0.0.0
LABEL org.opencontainers.image.title="family-frame-server"
LABEL org.opencontainers.image.version="$VERSION"
LABEL org.opencontainers.image.source="https://github.com/antoinejaussoin/family-frame"

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        wget \
        gnupg \
        fonts-liberation \
        fonts-dejavu-core \
        fonts-noto-core \
        fonts-noto-color-emoji \
    && ARCH="$(dpkg --print-architecture)" \
    && if [ "$ARCH" = "amd64" ]; then \
        wget -qO /tmp/google-chrome.deb https://dl.google.com/linux/direct/google-chrome-stable_current_amd64.deb \
        && apt-get install -y --no-install-recommends /tmp/google-chrome.deb \
        && rm -f /tmp/google-chrome.deb; \
       else \
        apt-get install -y --no-install-recommends chromium; \
       fi \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /src/target/release/eink-frame /usr/local/bin/eink-frame
COPY --from=ui /ui/dist /app/ui
COPY fixtures /app/fixtures
COPY config.example.toml /app/config.example.toml
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

ENV EINK_HOME=/app
ENV EINK_UI=/app/ui
ENV RUST_LOG=eink_frame=info,tower_http=info

EXPOSE 8765

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s \
    CMD wget -qO- http://127.0.0.1:8765/health || exit 1

ENTRYPOINT ["docker-entrypoint.sh"]
CMD ["eink-frame"]
