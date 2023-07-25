# syntax=docker/dockerfile:1

FROM rust:1.70

WORKDIR /app
COPY . .

RUN cargo build --release

ENTRYPOINT [ "/target/release/kingland-website" ]

EXPOSE 80/tcp
EXPOSE 443/tcp