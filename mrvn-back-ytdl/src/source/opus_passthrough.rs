use songbird::input::reader::MediaSource;
use std::io;
use std::io::{Read, Seek, SeekFrom};
use symphonia::core::formats::{FormatReader, Packet};

enum State {
    Header,
    Frame { packet: Packet, offset: usize },
}

pub struct OpusPassthroughSource {
    reader: Box<dyn FormatReader>,
    track_id: u32,
    state: State,
}

impl OpusPassthroughSource {
    pub fn new(reader: Box<dyn FormatReader>, track_id: u32) -> Self {
        OpusPassthroughSource {
            reader,
            track_id,
            state: State::Header,
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
}

impl MediaSource for OpusPassthroughSource {
    fn is_seekable(&self) -> bool {
        false
    }

    fn len(&self) -> Option<u64> {
        None
    }
}

impl Read for OpusPassthroughSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match &mut self.state {
            State::Header => {
                // Read the next valid packet for this track
                let packet = self.next_packet()?;

                // Write the packet header (packet len as a u16)
                // This will panic if buf.len() is less than size_of::<u16>()!
                let header_bytes = (packet.data.len() as u16).to_ne_bytes();
                let transfer_len = header_bytes.len();
                buf[..transfer_len].copy_from_slice(&header_bytes);

                // Next read call will return frame data
                self.state = State::Frame { packet, offset: 0 };

                Ok(transfer_len)
            }
            State::Frame { packet, offset } => {
                // Copy from the remaining data in the packet
                let remaining_data = &packet.data[*offset..];
                let transfer_len = buf.len().min(remaining_data.len());
                buf[..transfer_len].copy_from_slice(&remaining_data[..transfer_len]);

                *offset += transfer_len;
                debug_assert!(*offset <= packet.data.len());

                // If this was the end of the packet, send the header next
                if *offset == packet.data.len() {
                    self.state = State::Header;
                }

                Ok(transfer_len)
            }
        }
    }
}

impl Seek for OpusPassthroughSource {
    fn seek(&mut self, _pos: SeekFrom) -> io::Result<u64> {
        // No!
        panic!("Attempting to seek on non-seekable streaming source")
    }
}
