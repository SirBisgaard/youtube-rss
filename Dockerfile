FROM rust:1-slim AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src src
COPY static static
RUN cargo build --release

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/target/release/youtube-rss /usr/local/bin/youtube-rss
EXPOSE 8080
CMD ["youtube-rss"]
