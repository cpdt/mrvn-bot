# ![MRVN smiley face](mrvn.png) MRVN

MRVN is a Discord music player bot. It has a few neat features:

 - Can play videos and streams from Youtube, Soundcloud, Twitch, and
   [many more](https://ytdl-org.github.io/youtube-dl/supportedsites.html).
 - Exclusively uses Discord application commands.
 - Multi-channel support: allows simultaneous playback in multiple channels by
   using multiple bot applications.
 - Per-user queues: your queued songs follow you between channels. Each bot
   alternates between songs queued by people in the channel, so nobody misses
   out.

## What works

MRVN is currently under development. The following features are available:

 - `/play [query or url]` adds a song to your queue and starts playback in the
   channel if required.
 - `/pause` pauses the current song playing your voice channel.
 - `/play` unpauses the current song, or makes the bot starting if you have
   previously queued songs.
 - `/skip` skips the current song, or votes to skip if it you weren't the
   original queue-er. The number of votes needed is configurable.
 - `/stop` skips the current song and doesn't play any more queued songs. Use
   `/play` to continue playback.
 - `/replace` replaces your most recently queued song.
 - Queue management is not implemented yet.

## Set up

MRVN is self-hosted. This means you must register your own Discord applications
and run the bot on your own system. It's written in
[Rust](https://www.rust-lang.org/) and runs on Windows, Linux and macOS. You
can build MRVN yourself or use our prebuilt Docker images, but either way you
must set up the Discord application first:

### Creating the Discord Applications

1. Open the [Discord Developer Portal](https://discord.com/developers) and
   create an application for each channel you want to be able to play
   simultaneously. E.g. if your guild has three voice channels, you might want
   three applications to be able to listen to music in all channels at the same
   time.
2. Ensure you create a Bot user for each application. You can do this from the
   "Bot" panel in the application settings.
3. Download a copy of the [config.example.json](https://github.com/cpdt/mrvn-bot/blob/master/config.example.json)
   file and save it somewhere, maybe as config.json. This file contains your 
   configuration for the bot, including Discord application tokens.
4. Open the new config.json file and add the bot token and application ID for
   each Discord application. The "command bot" is the one that has application
   commands registered against it. It can be one of the voice bots, but you must
   also include it in the voice bot list.
5. Add each bot user to your Discord guild:
    - Visit the following URL to add the command bot, replacing
      `APPLICATION_ID_HERE` with the bots application ID:
      `https://discord.com/oauth2/authorize?client_id=APPLICATION_ID_HERE&scope=bot%20applications.commands&permissions=3145728`
    - Visit the following URL to add each non-command bot, again replacing
      `APPLICATION_ID_HERE` with the bots application ID:
      `https://discord.com/oauth2/authorize?client_id=APPLICATION_ID_HERE&scope=bot&permissions=3145728`
    - The different between these is because the command bot needs to request
      extra permissions to create application commands.

### Run the Docker image (recommended)

 1. Follow the steps to [install the Docker Engine](https://docs.docker.com/engine/install/).
 2. Once installed, run the following from a command prompt, replacing 
    `path/to/config.json` with the path to your configuration saved in the
    previous section: `docker run --name mrvn-bot --rm --mount type=bind,source=path/to/config.json,target=/config.json ghcr.io/cpdt/mrvn-bot:latest`
 3. You can stop MRVN by running `docker stop mrvn-bot`

### Build and run locally

This is an alternative to running the Docker image as described above. I would
recommend you follow those instructions as they involve less setting up, but
you're welcome to build and run MRVN yourself:

 1. First install some dependencies:
    - [Git](https://git-scm.com/)
    - [Rustup](https://rustup.rs/)
    - [youtube-dl](https://youtube-dl.org/)
    - [FFmpeg](https://www.ffmpeg.org)
 2. Clone the repository by running `git clone https://github.com/cpdt/mrvn-bot`
 3. Inside the repository, run the following from a command prompt, replacing
    `path/to/config.json` with the path to your configuration saved in the
    previous section: `cargo run --release path/to/config.json`
 4. You can stop MRVN by pressing Ctrl+C in the terminal window.

## Why?

In mid-2021 [Groovy](https://groovy.bot) and [Rythm](https://rythm.fm), Discord’s two largest music bots, were taken offline by YouTube. In the wake of this, I created MRVN mainly to serve a couple of servers I’m in, but also as an open tool for anyone looking for a new music bot.

Unlike Groovy, Rythm and many like them, MRVN was designed from the ground up to be self-hosted and simple to setup, making it impossible to be taken down as a whole. As open source software MRVN also does not charge its users, following the most notable clause in YouTube’s terms of service that commercial music bots break.

Finally, as a personal project MRVN has been an opportunity for me to re-think how my friends and I use music bots. This has led to what I consider improvements over the old formula: playing songs in a round-robin pattern so everybody gets a go, handling servers with multiple voice channels with a breeze, and codifying the unspoken “you shall not skip a song that is not yours” rule.

Please reach out and let me know if you’re using MRVN on your server! I would love to hear what works and what doesn’t, and see how it’s being used “in the wild”.

## License

MRVN is available under the [MIT license](https://opensource.org/licenses/MIT).
See the LICENSE file for details.

The MRVN smiley face used in this document is sourced from the [Titanfall Wiki](https://titanfall.fandom.com/wiki/Mk._III_Mobile_Robotic_Versatile_Entity_Automated_Assistant) and is copyright Respawn Entertainment 2014.
