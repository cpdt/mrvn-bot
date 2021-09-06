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
 - `/replace` replaces your most recently queued song.
 - `/queue` and `/stop` are not implemented yet.

## Set up

MRVN is self-hosted. This means you must register your own Discord applications
and run the bot on your own system. It's written in
[Rust](https://www.rust-lang.org/) and runs on Windows, Linux and macOS.

First some things you'll need to install:

 - [Git](https://git-scm.com/)
 - [Rustup](https://rustup.rs/)
 - [youtube-dl](https://youtube-dl.org/)
 - [FFmpeg](https://www.ffmpeg.org)

Follow these steps to set MRVN up for the first time:

 1. Open the [Discord Developer Portal](https://discord.com/developers) and
    create an application for each channel you want to be able to play
    simultaneously. E.g. if your guild has three voice channels, you might want
    three applications to be able to listen to music in all channels at the same
    time.
 2. Ensure you create a Bot user for each application. You can do this from the
    "Bot" panel in the application settings.
 3. Clone this repository to your computer. In a terminal window enter
    `git clone https://github.com/cpdt/mrvn-bot`, which after running will
    create a `mrvn-bot` folder.
 4. In the terminal window, enter the `mrvn-bot/mrvn-front-discord` folder:
    `cd mrvn-bot/mrvn-front-discord`.
 5. Build the bot by running `cargo build --release`.
 6. Back in the `mrvn-bot` folder, copy the `config.example.json` file to a new
    file called `config.json`. Open the new file and add the bot token and
    application ID for each Discord application. The "command bot" is the one
    that has application commands registered against it, and can be one of the
    voice bots.
 7. Add each bot user to your Discord guild:
     - Visit the following URL to add the command bot, replacing
       `APPLICATION_ID_HERE` with the bots application ID:
       `https://discord.com/oauth2/authorize?client_id=APPLICATION_ID_HERE&scope=bot%20applications.commands&permissions=3145728`
     - Visit the following URL to add each non-command bot, again replacing
       `APPLICATION_ID_HERE` with the bots application ID:
       `https://discord.com/oauth2/authorize?client_id=APPLICATION_ID_HERE&scope=bot&permissions=3145728`
     - The different between these is because the command bot needs to request
       extra permissions to create application commands.

Now that MRVN is all set up, follow these steps to run it:

 1. Set the `RUST_LOG` environment variable to `mrvn` to see logs in the
    terminal window. In the windows command prompt, run `SET RUST_LOG=mrvn`.
    In a Bash shell run `export RUST_LOG=mrvn`. This environment variable
    follows the format used with [env_logger](https://docs.rs/env_logger).
 2. From the `mrvn-bot/mrvn-front-discord` folder, run the command
    `cargo run --release ../config.json`. This will start the bot.
 3. You can stop the bot at any time by pressing Ctrl+C in the terminal window.

## License

MRVN is available under the [MIT license](https://opensource.org/licenses/MIT).
See the LICENSE file for details.

The MRVN smiley face used in this document is sourced from the [Titanfall Wiki](https://titanfall.fandom.com/wiki/Mk._III_Mobile_Robotic_Versatile_Entity_Automated_Assistant) and is copyright Respawn Entertainment 2014.
