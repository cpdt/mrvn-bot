use adts_reader::{
    AdtsConsumer, AdtsParseError, AdtsParser, AudioObjectType, ChannelConfiguration, MpegVersion,
    Originality, ProtectionIndicator, SamplingFrequency,
};
use mpeg2ts_reader::demultiplex::{Demultiplex, DemuxContext, FilterChangeset, FilterRequest};
use mpeg2ts_reader::pes::PesHeader;
use mpeg2ts_reader::{demultiplex, packet_filter_switch, pes, psi, StreamType};
use std::collections::VecDeque;
use std::io;
use std::io::Read;
use symphonia::core::audio::{Channels, Layout};
use symphonia::core::codecs::{CodecParameters, CODEC_TYPE_AAC};
use symphonia::core::errors::SeekErrorKind;
use symphonia::core::formats::{
    Cue, FormatOptions, FormatReader, Packet, SeekMode, SeekTo, SeekedTo, Track,
};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{Metadata, MetadataLog};
use symphonia::core::units::TimeBase;

const AAC_SAMPLES_PER_BLOCK: u32 = 1024;
const READ_BUF_LEN: usize = mpeg2ts_reader::packet::Packet::SIZE;
const READ_TRACKS_TIMEOUT_BYTES: usize = mpeg2ts_reader::packet::Packet::SIZE * 4096;

pub struct MpegTsReader {
    reader: MediaSourceStream,
    metadata: MetadataLog,

    ctx: ReadAudioDemuxContext,
    demux: Demultiplex<ReadAudioDemuxContext>,

    read_buf: [u8; READ_BUF_LEN],
}

packet_filter_switch! {
    ReadAudioFilterSwitch<ReadAudioDemuxContext> {
        AdtsPes: pes::PesPacketFilter<ReadAudioDemuxContext, AdtsElementaryStreamConsumer>,
        UnknownPes: pes::PesPacketFilter<ReadAudioDemuxContext, UnknownElementaryStreamConsumer>,

        Pat: demultiplex::PatPacketFilter<ReadAudioDemuxContext>,
        Pmt: demultiplex::PmtPacketFilter<ReadAudioDemuxContext>,

        Null: demultiplex::NullPacketFilter<ReadAudioDemuxContext>,
    }
}

pub struct ReadAudioDemuxContext {
    changeset: FilterChangeset<ReadAudioFilterSwitch>,

    stream_count: usize,
    has_started_any_stream: bool,
    tracks: Vec<Track>,

    packets: VecDeque<symphonia::core::errors::Result<Packet>>,
}

impl ReadAudioDemuxContext {
    pub fn new() -> Self {
        ReadAudioDemuxContext {
            changeset: Default::default(),

            stream_count: 0,
            has_started_any_stream: false,
            tracks: Vec::new(),

            packets: VecDeque::new(),
        }
    }
}

impl DemuxContext for ReadAudioDemuxContext {
    type F = ReadAudioFilterSwitch;

    fn filter_changeset(&mut self) -> &mut FilterChangeset<Self::F> {
        &mut self.changeset
    }

    fn construct(&mut self, req: FilterRequest<'_, '_>) -> Self::F {
        match req {
            // Use the default handler for the Program Association Table.
            demultiplex::FilterRequest::ByPid(mpeg2ts_reader::psi::pat::PAT_PID) => {
                ReadAudioFilterSwitch::Pat(demultiplex::PatPacketFilter::default())
            }
            // Ignore stuffing data.
            demultiplex::FilterRequest::ByPid(mpeg2ts_reader::STUFFING_PID) => {
                ReadAudioFilterSwitch::Null(demultiplex::NullPacketFilter::default())
            }
            // Ignore other PIDs that weren't announced in the metadata.
            demultiplex::FilterRequest::ByPid(_) => {
                ReadAudioFilterSwitch::Null(demultiplex::NullPacketFilter::default())
            }
            // Handle ADTS streams.
            demultiplex::FilterRequest::ByStream {
                stream_type: StreamType::Adts,
                pmt,
                stream_info,
                ..
            } => {
                self.stream_count += 1;
                ReadAudioFilterSwitch::AdtsPes(AdtsElementaryStreamConsumer::new(pmt, stream_info))
            }
            // Ignore unknown streams, but use them to tell if any streams have started.
            demultiplex::FilterRequest::ByStream { .. } => {
                ReadAudioFilterSwitch::UnknownPes(UnknownElementaryStreamConsumer::new())
            }
            // Use the default handler for the Program Map Table.
            demultiplex::FilterRequest::Pmt {
                pid,
                program_number,
            } => ReadAudioFilterSwitch::Pmt(demultiplex::PmtPacketFilter::new(pid, program_number)),
            // Ignore the Network Information Table.
            demultiplex::FilterRequest::Nit { .. } => {
                ReadAudioFilterSwitch::Null(demultiplex::NullPacketFilter::default())
            }
        }
    }
}

pub struct UnknownElementaryStreamConsumer;

impl UnknownElementaryStreamConsumer {
    fn new() -> pes::PesPacketFilter<ReadAudioDemuxContext, Self> {
        pes::PesPacketFilter::new(UnknownElementaryStreamConsumer)
    }
}

impl pes::ElementaryStreamConsumer<ReadAudioDemuxContext> for UnknownElementaryStreamConsumer {
    fn start_stream(&mut self, ctx: &mut ReadAudioDemuxContext) {
        ctx.has_started_any_stream = true;
    }

    fn begin_packet(&mut self, _ctx: &mut ReadAudioDemuxContext, _header: PesHeader<'_>) {}

    fn continue_packet(&mut self, _ctx: &mut ReadAudioDemuxContext, _data: &[u8]) {}

    fn end_packet(&mut self, _ctx: &mut ReadAudioDemuxContext) {}

    fn continuity_error(&mut self, _ctx: &mut ReadAudioDemuxContext) {}
}

pub struct AdtsElementaryStreamConsumer {
    track_id: u32,
    parser: AdtsParser<AdtsDataConsumer>,

    codec_params: Option<CodecParameters>,
    track_index: Option<usize>,

    ts: u64,
}

impl AdtsElementaryStreamConsumer {
    fn new(
        _pmt_sect: &psi::pmt::PmtSection,
        stream_info: &psi::pmt::StreamInfo,
    ) -> pes::PesPacketFilter<ReadAudioDemuxContext, Self> {
        pes::PesPacketFilter::new(AdtsElementaryStreamConsumer {
            track_id: u16::from(stream_info.elementary_pid()) as u32,

            parser: AdtsParser::new(AdtsDataConsumer {
                codec_params: None,
                buffers: Vec::new(),
            }),

            codec_params: None,
            track_index: None,

            ts: 0,
        })
    }
}

impl pes::ElementaryStreamConsumer<ReadAudioDemuxContext> for AdtsElementaryStreamConsumer {
    fn start_stream(&mut self, ctx: &mut ReadAudioDemuxContext) {
        ctx.has_started_any_stream = true;
    }

    fn begin_packet(&mut self, _ctx: &mut ReadAudioDemuxContext, header: PesHeader<'_>) {
        self.parser.start();

        match header.contents() {
            pes::PesContents::Parsed(Some(parsed)) => {
                self.parser.push(parsed.payload());
            }
            pes::PesContents::Parsed(None) => {}
            pes::PesContents::Payload(payload) => {
                self.parser.push(payload);
            }
        }
    }

    fn continue_packet(&mut self, _ctx: &mut ReadAudioDemuxContext, data: &[u8]) {
        self.parser.push(data);
    }

    fn end_packet(&mut self, ctx: &mut ReadAudioDemuxContext) {
        let consumer = &mut self.parser.consumer;
        let mut did_params_change = false;

        // If the parser has new params, replace our existing params
        if let Some(codec_params) = consumer.codec_params.take() {
            self.codec_params = Some(codec_params);
            did_params_change = true;
        }

        // We can't continue until we have some params
        let codec_params = match &self.codec_params {
            Some(codec_params) => codec_params,
            None => return,
        };

        if let Some(track_index) = self.track_index {
            // Update the existing track codec params if required
            if did_params_change {
                ctx.tracks[track_index].codec_params = codec_params.clone();
            }
        } else {
            // Create a new track if one doesn't exist
            self.track_index = Some(ctx.tracks.len());
            ctx.tracks.push(Track {
                id: self.track_id,
                codec_params: codec_params.clone(),
                language: None,
            });
        }

        // Emit packets back to the context
        ctx.packets.extend(
            consumer
                .buffers
                .drain(..)
                .map(|maybe_buffer| match maybe_buffer {
                    Ok(AdtsBuffer { block_count, data }) => {
                        let ts = self.ts;
                        let dur = block_count as u64;
                        self.ts += dur;

                        Ok(Packet::new_from_boxed_slice(self.track_id, ts, dur, data))
                    }
                    Err(AdtsParseError::BadFrameLength) => Err(
                        symphonia::core::errors::Error::DecodeError("bad frame length"),
                    ),
                    Err(AdtsParseError::BadSyncWord) => {
                        Err(symphonia::core::errors::Error::DecodeError("bad sync word"))
                    }
                }),
        );
    }

    fn continuity_error(&mut self, _ctx: &mut ReadAudioDemuxContext) {
        // todo: should this be handled
    }
}

struct AdtsBuffer {
    block_count: u8,
    data: Box<[u8]>,
}

struct AdtsDataConsumer {
    codec_params: Option<CodecParameters>,
    buffers: Vec<Result<AdtsBuffer, AdtsParseError>>,
}

impl AdtsConsumer for AdtsDataConsumer {
    fn new_config(
        &mut self,
        _mpeg_version: MpegVersion,
        _protection: ProtectionIndicator,
        _aot: AudioObjectType,
        freq: SamplingFrequency,
        _private_bit: u8,
        channel_config: ChannelConfiguration,
        _originality: Originality,
        _home: u8,
    ) {
        let channels = match channel_config {
            ChannelConfiguration::ObjectTypeSpecificConfig => None,
            ChannelConfiguration::Mono => Some(Channels::FRONT_CENTRE),
            ChannelConfiguration::Stereo => Some(Channels::FRONT_LEFT | Channels::FRONT_RIGHT),
            ChannelConfiguration::Three => {
                Some(Channels::FRONT_CENTRE | Channels::FRONT_LEFT | Channels::FRONT_RIGHT)
            }
            ChannelConfiguration::Four => Some(
                Channels::FRONT_CENTRE
                    | Channels::FRONT_LEFT
                    | Channels::FRONT_RIGHT
                    | Channels::REAR_CENTRE,
            ),
            ChannelConfiguration::Five => Some(
                Channels::FRONT_CENTRE
                    | Channels::FRONT_LEFT
                    | Channels::FRONT_RIGHT
                    | Channels::REAR_LEFT
                    | Channels::REAR_RIGHT,
            ),
            ChannelConfiguration::FiveOne => Some(
                Channels::FRONT_CENTRE
                    | Channels::FRONT_LEFT
                    | Channels::FRONT_RIGHT
                    | Channels::REAR_LEFT
                    | Channels::REAR_RIGHT
                    | Channels::LFE1,
            ),
            ChannelConfiguration::SevenOne => Some(
                Channels::FRONT_CENTRE
                    | Channels::FRONT_LEFT
                    | Channels::FRONT_RIGHT
                    | Channels::SIDE_LEFT
                    | Channels::SIDE_RIGHT
                    | Channels::REAR_LEFT
                    | Channels::REAR_RIGHT
                    | Channels::LFE1,
            ),
        };
        let channel_layout = match channel_config {
            ChannelConfiguration::ObjectTypeSpecificConfig => None,
            ChannelConfiguration::Mono => Some(Layout::Mono),
            ChannelConfiguration::Stereo => Some(Layout::Stereo),
            ChannelConfiguration::Three => None,
            ChannelConfiguration::Four => None,
            ChannelConfiguration::Five => None,
            ChannelConfiguration::FiveOne => Some(Layout::FivePointOne),
            ChannelConfiguration::SevenOne => None,
        };

        self.codec_params = Some(CodecParameters {
            codec: CODEC_TYPE_AAC,
            sample_rate: freq.freq(),
            time_base: freq
                .freq()
                .map(|freq| TimeBase::new(AAC_SAMPLES_PER_BLOCK, freq)),
            n_frames: None,
            start_ts: 0,
            sample_format: None,
            bits_per_sample: None,
            bits_per_coded_sample: None,
            channels,
            channel_layout,
            delay: None,
            padding: None,
            max_frames_per_packet: None,
            packet_data_integrity: false,
            verification_check: None,
            extra_data: None,
        });
    }

    fn payload(&mut self, _buffer_fullness: u16, number_of_blocks: u8, buf: &[u8]) {
        self.buffers.push(Ok(AdtsBuffer {
            block_count: number_of_blocks,
            data: buf.into(),
        }));
    }

    fn error(&mut self, err: AdtsParseError) {
        self.buffers.push(Err(err));
    }
}

impl FormatReader for MpegTsReader {
    fn try_new(
        mut source: MediaSourceStream,
        _options: &FormatOptions,
    ) -> symphonia::core::errors::Result<Self>
    where
        Self: Sized,
    {
        let mut ctx = ReadAudioDemuxContext::new();
        let mut demux = demultiplex::Demultiplex::new(&mut ctx);

        let mut total_bytes = 0;

        // Read until all declared tracks have started.
        // This requires:
        //  - ctx.has_started_any_stream must be true
        //  - ctx.tracks.len() must be >= ctx.stream_count
        //  - us to pass the timeout
        let mut buf = [0u8; READ_BUF_LEN];
        while (!ctx.has_started_any_stream || ctx.tracks.len() < ctx.stream_count)
            && total_bytes < READ_TRACKS_TIMEOUT_BYTES
        {
            match source.read(&mut buf) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into()),
                Ok(read_bytes) => {
                    demux.push(&mut ctx, &buf[..read_bytes]);
                    total_bytes += read_bytes;
                }
                Err(why) => return Err(why.into()),
            }
        }

        Ok(MpegTsReader {
            reader: source,
            metadata: MetadataLog::default(),
            ctx,
            demux,
            read_buf: [0; READ_BUF_LEN],
        })
    }

    fn cues(&self) -> &[Cue] {
        Default::default()
    }

    fn metadata(&mut self) -> Metadata<'_> {
        self.metadata.metadata()
    }

    fn seek(&mut self, _mode: SeekMode, _to: SeekTo) -> symphonia::core::errors::Result<SeekedTo> {
        Err(symphonia::core::errors::Error::SeekError(
            SeekErrorKind::Unseekable,
        ))
    }

    fn tracks(&self) -> &[Track] {
        &self.ctx.tracks
    }

    fn next_packet(&mut self) -> symphonia::core::errors::Result<Packet> {
        loop {
            match self.ctx.packets.pop_front() {
                Some(maybe_packet) => return maybe_packet,
                None => {
                    match self.reader.read(&mut self.read_buf) {
                        Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into()),
                        Ok(read_bytes) => {
                            self.demux.push(&mut self.ctx, &self.read_buf[..read_bytes])
                        }
                        Err(why) => return Err(why.into()),
                    }
                    // log::trace!("{} new packets", self.ctx.packets.len());
                }
            }
        }
    }

    fn into_inner(self: Box<Self>) -> MediaSourceStream {
        self.reader
    }
}
