FROM --platform=$BUILDPLATFORM docker.io/library/rust@sha256:7c4ae649a84014c467d79319bbf17ce2632ae8b8be123ac2fb2ea5be46823f31 AS frontend
ARG BUILDARCH
RUN test "$BUILDARCH" = amd64
RUN printf '%s\n' \
      'deb [check-valid-until=no] https://snapshot.debian.org/archive/debian/20260801T000000Z bookworm main' \
      > /etc/apt/sources.list \
    && rm -f /etc/apt/sources.list.d/* \
    && apt-get -o Acquire::Check-Valid-Until=false update \
    && apt-get install --yes --no-install-recommends \
      ca-certificates=20230311+deb12u1 \
      clang=1:14.0-55.7~deb12u1 \
      curl=7.88.1-10+deb12u15 \
    && rm -rf /var/lib/apt/lists/* \
    && rustup target add wasm32-unknown-unknown
RUN archive=/tmp/cargo-leptos.tar.gz \
    && curl --fail --location --silent --show-error --output "$archive" \
      https://github.com/leptos-rs/cargo-leptos/releases/download/v0.3.7/cargo-leptos-x86_64-unknown-linux-gnu.tar.gz \
    && printf '%s  %s\n' fda80f4845e92d0e8f5ec13cf1a46982ba7a518ae01182e7e4201312944bc05d "$archive" \
      | sha256sum --check --strict \
    && tar --extract --gzip --file "$archive" --directory /usr/local/bin --strip-components=1 \
      cargo-leptos-x86_64-unknown-linux-gnu/cargo-leptos \
    && rm "$archive" \
    && test "$(cargo leptos --version)" = 'cargo-leptos 0.3.7'
WORKDIR /source
COPY . .
RUN cargo leptos build --frontend-only --release --lib-cargo-args=--locked \
    && test -d target/site \
    && mkdir -p /out \
    && cp -a target/site /out/site

FROM --platform=$BUILDPLATFORM docker.io/library/rust@sha256:7c4ae649a84014c467d79319bbf17ce2632ae8b8be123ac2fb2ea5be46823f31 AS backend
ARG TARGETARCH
RUN test "$TARGETARCH" = amd64 -o "$TARGETARCH" = arm64
RUN if test "$TARGETARCH" = arm64; then \
      printf '%s\n' \
        'deb [check-valid-until=no] https://snapshot.debian.org/archive/debian/20260801T000000Z bookworm main' \
        > /etc/apt/sources.list \
      && rm -f /etc/apt/sources.list.d/* \
      && apt-get -o Acquire::Check-Valid-Until=false update \
      && apt-get install --yes --no-install-recommends \
        gcc-aarch64-linux-gnu=4:12.2.0-3 \
        libc6-dev-arm64-cross=2.36-8cross1 \
      && rm -rf /var/lib/apt/lists/*; \
    fi
RUN target="$(if test "$TARGETARCH" = amd64; then printf x86_64-unknown-linux-gnu; else printf aarch64-unknown-linux-gnu; fi)" \
    && rustup target add "$target"
WORKDIR /source
COPY . .
RUN target="$(if test "$TARGETARCH" = amd64; then printf x86_64-unknown-linux-gnu; else printf aarch64-unknown-linux-gnu; fi)" \
    && if test "$TARGETARCH" = arm64; then \
      CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
        CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
        cargo build --locked --release --target "$target"; \
    else \
      cargo build --locked --release --target "$target"; \
    fi \
    && install -D -m 0755 "target/$target/release/print-manager" /out/print-manager

FROM docker.io/library/debian@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171
RUN printf '%s\n' \
      'deb [check-valid-until=no] http://snapshot.debian.org/archive/debian/20260801T000000Z bookworm main' \
      > /etc/apt/sources.list \
    && rm -f /etc/apt/sources.list.d/* \
    && apt-get -o Acquire::Check-Valid-Until=false update \
    && apt-get install --yes --no-install-recommends \
      ca-certificates=20230311+deb12u1 \
    && rm -rf /var/lib/apt/lists/* \
    && install -d -m 0700 -o 65532 -g 65532 /data
WORKDIR /app
COPY --from=backend /out/print-manager /usr/local/bin/print-manager
COPY --from=frontend /out/site /app/site
ENV DATABASE_URL=sqlite:/data/print-manager.db \
    BIND_ADDR=0.0.0.0:3000 \
    LEPTOS_SITE_ROOT=/app/site \
    PRINT_MANAGER_DATA_ROOT=/data \
    SHOP_TIME_ZONE=America/New_York \
    RUST_LOG=info
EXPOSE 3000
VOLUME ["/data"]
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/print-manager"]
