use byte_slice_cast::AsByteSlice;
use rubato::{FftFixedInOut, Resampler};
use songbird::constants::SAMPLE_RATE_RAW;
use songbird::input::reader::MediaSource;
use std::io;
use std::io::{Read, Seek, SeekFrom};
use std::mem::size_of;
use symphonia::core::audio::{AudioBuffer, AudioBufferRef, Signal};
use symphonia::core::codecs::Decoder;
use symphonia::core::conv::IntoSample;
use symphonia::core::formats::{FormatReader, Packet};
use symphonia::core::sample::Sample;

pub struct DecodedPcmSource {
    decoder_source: DecoderSource,

    decode_offset: usize,
    interleaved_byte_len: usize,
    interleaved_byte_offset: usize,

    resample: FftFixedInOut<f32>,
    not_resampled: Vec<Vec<f32>>,
    resampled: Vec<Vec<f32>>,
    interleaved: Vec<f32>,
}

impl DecodedPcmSource {
    pub fn new(
        reader: Box<dyn FormatReader>,
        decoder: Box<dyn Decoder>,
        track_id: u32,
        is_stereo: bool,
    ) -> Result<Self, crate::Error> {
        let resample = FftFixedInOut::new(
            decoder.codec_params().sample_rate.unwrap() as usize,
            SAMPLE_RATE_RAW,
            64,
            if is_stereo { 2 } else { 1 },
        )
        .map_err(crate::Error::RubatoConstruction)?;

        let not_resampled = resample.input_buffer_allocate();
        let resampled = resample.output_buffer_allocate();
        let interleaved = vec![0.; resample.output_frames_max() * resample.nbr_channels()];

        Ok(DecodedPcmSource {
            decoder_source: DecoderSource::new(reader, decoder, track_id),

            decode_offset: 0,
            interleaved_byte_len: 0,
            interleaved_byte_offset: 0,

            resample,
            not_resampled,
            resampled,
            interleaved,
        })
    }

    fn next_resampled_chunk(&mut self) -> io::Result<()> {
        let chunk_frames = self.resample.input_frames_next();

        // Prepare each not_resampled buffer to receive D A T A
        for buffer in &mut self.not_resampled {
            buffer.resize(chunk_frames, 0.);
        }

        // Number of frames that have been copied to this chunk so far
        let mut input_offset = 0;

        // Keep decoding packets until we fill the not_resampled buffer
        while input_offset < chunk_frames {
            let decode_buffer = self.decoder_source.read_decode_buffer()?;
            let decode_available_frames = decode_buffer.frames();
            let decode_remaining_frames = decode_available_frames - self.decode_offset;
            let copy_frames = (chunk_frames - input_offset).min(decode_remaining_frames);

            // Copy frames as required, converting to floats if necessary
            for (channel, dest_buffer) in self.not_resampled.iter_mut().enumerate() {
                let dest_slice = &mut dest_buffer[input_offset..(input_offset + copy_frames)];
                copy_buffer_ref(&decode_buffer, dest_slice, channel, self.decode_offset);
            }

            self.decode_offset += copy_frames;
            debug_assert!(self.decode_offset <= decode_available_frames);

            // If we have now read the entire decode buffer, clear it so next time we fetch a new
            // one.
            if self.decode_offset == decode_available_frames {
                self.decode_offset = 0;
                self.decoder_source.consume();
            }

            input_offset += copy_frames;
            debug_assert!(input_offset <= chunk_frames);
        }

        let output_frames = self.resample.output_frames_next();

        // Hol' up, gotta process
        self.resample
            .process_into_buffer(&self.not_resampled, &mut self.resampled, None)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

        // Get our interleaved buffer ready
        copy_interleaved(&self.resampled, &mut self.interleaved, output_frames);
        self.interleaved_byte_len = output_frames * self.resample.nbr_channels() * size_of::<f32>();
        self.interleaved_byte_offset = 0;

        // And that's all folks
        Ok(())
    }
}

impl MediaSource for DecodedPcmSource {
    fn is_seekable(&self) -> bool {
        false
    }

    fn len(&self) -> Option<u64> {
        None
    }
}

impl Read for DecodedPcmSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.interleaved_byte_offset == self.interleaved_byte_len {
            // Looks like we're gonna need a new resampled buffer, chief
            self.next_resampled_chunk()?;
        }

        let interleaved_byte_len = self.interleaved_byte_len;

        let src_buf =
            &self.interleaved.as_byte_slice()[self.interleaved_byte_offset..interleaved_byte_len];
        let copy_len = buf.len().min(src_buf.len());
        buf[..copy_len].copy_from_slice(&src_buf[..copy_len]);

        self.interleaved_byte_offset += copy_len;
        debug_assert!(self.interleaved_byte_offset <= interleaved_byte_len);

        Ok(copy_len)
    }
}

impl Seek for DecodedPcmSource {
    fn seek(&mut self, _pos: SeekFrom) -> io::Result<u64> {
        // No!
        panic!("Attempting to seek on non-seekable streaming source")
    }
}

struct DecoderSource {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,

    has_consumed_packet: bool,
}

impl DecoderSource {
    fn new(reader: Box<dyn FormatReader>, decoder: Box<dyn Decoder>, track_id: u32) -> Self {
        DecoderSource {
            reader,
            decoder,
            track_id,

            // force the first `next_decode_buffer` call to load a new buffer
            has_consumed_packet: true,
        }
    }

    fn next_packet(&mut self) -> io::Result<Packet> {
        loop {
            let packet = self
                .reader
                .next_packet()
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

            if packet.track_id() == self.track_id {
                return Ok(packet);
            }
        }
    }

    fn next_decode_buffer(&mut self) -> io::Result<AudioBufferRef> {
        loop {
            let packet = self.next_packet()?;

            match self.decoder.decode(&packet) {
                Ok(_) => return Ok(self.decoder.last_decoded()),
                Err(symphonia::core::errors::Error::IoError(err)) => {
                    log::warn!("Error while decoding buffer: {}", err);

                    // Skip this packet instead of bailing
                    continue;
                }
                Err(symphonia::core::errors::Error::DecodeError(err)) => {
                    log::warn!("Error while decoding buffer: {}", err);

                    // Skip this packet instead of bailing
                    continue;
                }
                Err(err) => return Err(io::Error::new(io::ErrorKind::Other, err)),
            }
        }
    }

    fn read_decode_buffer(&mut self) -> io::Result<AudioBufferRef> {
        if !self.has_consumed_packet {
            return Ok(self.decoder.last_decoded());
        }

        self.has_consumed_packet = false;
        self.next_decode_buffer()
    }

    fn consume(&mut self) {
        self.has_consumed_packet = true;
    }
}

fn copy_buffer<S: Sample + IntoSample<f32>>(
    src_buf: &AudioBuffer<S>,
    dest: &mut [f32],
    channel: usize,
    src_offset: usize,
) {
    let chan_buf = &src_buf.chan(channel)[src_offset..];

    for (&src_sample, dest_sample) in chan_buf.iter().zip(dest.iter_mut()) {
        *dest_sample = src_sample.into_sample();
    }
}

fn copy_buffer_ref(
    buffer_ref: &AudioBufferRef,
    dest: &mut [f32],
    channel: usize,
    src_offset: usize,
) {
    match buffer_ref {
        AudioBufferRef::U8(buf) => copy_buffer(buf, dest, channel, src_offset),
        AudioBufferRef::U16(buf) => copy_buffer(buf, dest, channel, src_offset),
        AudioBufferRef::U24(buf) => copy_buffer(buf, dest, channel, src_offset),
        AudioBufferRef::U32(buf) => copy_buffer(buf, dest, channel, src_offset),
        AudioBufferRef::S8(buf) => copy_buffer(buf, dest, channel, src_offset),
        AudioBufferRef::S16(buf) => copy_buffer(buf, dest, channel, src_offset),
        AudioBufferRef::S24(buf) => copy_buffer(buf, dest, channel, src_offset),
        AudioBufferRef::S32(buf) => copy_buffer(buf, dest, channel, src_offset),
        AudioBufferRef::F32(buf) => copy_buffer(buf, dest, channel, src_offset),
        AudioBufferRef::F64(buf) => copy_buffer(buf, dest, channel, src_offset),
    }
}

fn copy_interleaved(src: &[Vec<f32>], dest: &mut [f32], frames: usize) {
    let channels = src.len();
    match channels {
        // No channels, do nothing
        0 => (),
        // Mono
        1 => {
            dest[..frames].copy_from_slice(&src[0][..frames]);
        }
        // Stereo
        2 => {
            let l_buf = &src[0];
            let r_buf = &src[1];

            for ((&l, &r), dest) in l_buf.iter().zip(r_buf).zip(dest.chunks_exact_mut(2)) {
                dest[0] = l;
                dest[1] = r;
            }
        }
        // 3+ channels
        _ => {
            for (chan, src_chan) in src.iter().enumerate() {
                let dest_chan_iter = dest[chan..].iter_mut().step_by(channels);

                for (&src, dest) in src_chan.iter().zip(dest_chan_iter) {
                    *dest = src;
                }
            }
        }
    }
}
