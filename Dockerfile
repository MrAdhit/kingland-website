# syntax=docker/dockerfile:1

FROM rust:1.70 AS builder

COPY . .

RUN cargo build --release

FROM debian:11-slim AS runner

COPY --from=builder /target/release/kingland-website /bin/kingland-website

ENTRYPOINT [ "/bin/kingland-website" ]

EXPOSE 80/tcp
EXPOSE 443/tcp