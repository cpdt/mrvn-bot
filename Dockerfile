FROM rust:buster as builder
RUN curl -L https://yt-dl.org/downloads/2021.12.17/youtube-dl -o /usr/local/bin/youtube-dl && chmod a+rx /usr/local/bin/youtube-dl
WORKDIR /usr/src/mrvn-bot
COPY . .
RUN cargo install --path ./mrvn-front-discord

FROM bitnami/minideb:buster
RUN apt-get update && apt-get install -y ca-certificates libopus0 libopus-dev python && rm -rf /var/lib/apt/lists/*
RUN update-ca-certificates
COPY --from=builder /usr/local/bin/youtube-dl /usr/local/bin/youtube-dl
COPY --from=builder /usr/local/cargo/bin/mrvn-front-discord /usr/local/bin/mrvn-front-discord
ENV RUST_LOG=mrvn
CMD ["mrvn-front-discord", "config.json"]
LABEL org.opencontainers.image.source="https://github.com/cpdt/mrvn-bot"
