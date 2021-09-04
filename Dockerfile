FROM rust:1.54 as builder
RUN curl -L https://yt-dl.org/downloads/2021.06.06/youtube-dl -o /usr/local/bin/youtube-dl && chmod a+rx /usr/local/bin/youtube-dl
WORKDIR /usr/src/mrvn-bot
COPY . .
RUN cargo install --path ./mrvn-front-discord

FROM debian:buster-slim
RUN apt-get update && apt-get install -y ffmpeg python && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/bin/youtube-dl /usr/local/bin/youtube-dl
COPY --from=builder /usr/local/cargo/bin/mrvn-front-discord /usr/local/bin/mrvn-front-discord
COPY config.json config.json
ENV RUST_LOG=mrvn
CMD ["mrvn-front-discord", "config.json"]
