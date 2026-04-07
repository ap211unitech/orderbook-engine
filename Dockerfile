FROM rust:1.94.0-slim AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
# Pre-fetch dependencies for layer caching
RUN mkdir src && echo 'fn main(){}' > src/main.rs && cargo build --release && rm -rf src
COPY src ./src
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/server /usr/local/bin/server
EXPOSE 8080
CMD ["server"]
