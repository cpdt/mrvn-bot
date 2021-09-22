FROM rust:slim-bullseye as builder
RUN curl -L https://yt-dl.org/downloads/2021.06.06/youtube-dl -o /usr/local/bin/youtube-dl && chmod a+rx /usr/local/bin/youtube-dl
WORKDIR /usr/src/mrvn-bot
RUN apt-get update && apt-get install -y libopus0 libopus-dev pkg-config ffmpeg && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo install --path ./mrvn-front-discord

FROM bitnami/minideb:latest
RUN apt-get update && apt-get install -y ca-certificates libopus0 libopus-dev ffmpeg && rm -rf /var/lib/apt/lists/*
RUN update-ca-certificates
COPY --from=builder /usr/local/bin/youtube-dl /usr/local/bin/youtube-dl
COPY --from=builder /usr/local/cargo/bin/mrvn-front-discord /usr/local/bin/mrvn-front-discord
ENV RUST_LOG=mrvn
CMD ["mrvn-front-discord", "config.json"]
LABEL org.opencontainers.image.source="https://github.com/cpdt/mrvn-bot"
