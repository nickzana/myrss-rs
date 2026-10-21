FROM rust:1.97-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY migrations ./migrations
COPY src ./src
COPY static ./static
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN groupadd --gid 1000 myrss \
 && useradd --uid 1000 --gid 1000 --home-dir /var/lib/myrss --create-home --shell /usr/sbin/nologin myrss \
 && mkdir -p /etc/myrss
COPY --from=build /src/target/release/myrss /usr/local/bin/myrss
COPY docker-entrypoint.sh /usr/local/bin/myrss-entrypoint
RUN chmod 755 /usr/local/bin/myrss-entrypoint
EXPOSE 8080
VOLUME ["/var/lib/myrss"]
# The default data_dir (./data) resolves under the mounted volume.
WORKDIR /var/lib/myrss
ENTRYPOINT ["myrss-entrypoint"]
CMD ["--config", "/etc/myrss/config.toml", "serve"]
