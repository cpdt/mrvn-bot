FROM rust:latest AS builder
RUN apt-get update && apt-get install -y cmake
RUN curl -L https://github.com/yt-dlp/yt-dlp/releases/download/2026.03.17/yt-dlp_linux -o /usr/local/bin/youtube-dl && chmod a+rx /usr/local/bin/youtube-dl
RUN curl -L https://dl.deno.land/release/v2.7.11/deno-x86_64-unknown-linux-gnu.zip -o /usr/local/bin/deno.zip && unzip /usr/local/bin/deno.zip -d /usr/local/bin && chmod a+rx /usr/local/bin/deno
WORKDIR /usr/src/mrvn-bot
COPY . .
RUN cargo install --path ./mrvn-front-discord

FROM debian:stable-slim
RUN apt-get update && apt-get install -y ca-certificates ffmpeg
RUN update-ca-certificates
COPY --from=builder /usr/local/bin/youtube-dl /usr/local/bin/youtube-dl
COPY --from=builder /usr/local/cargo/bin/mrvn-front-discord /usr/local/bin/mrvn-front-discord
COPY --from=builder /usr/local/bin/deno /usr/local/bin/deno
ENV RUST_LOG=mrvn
CMD ["mrvn-front-discord", "config.json"]
LABEL org.opencontainers.image.source="https://github.com/cpdt/mrvn-bot"
