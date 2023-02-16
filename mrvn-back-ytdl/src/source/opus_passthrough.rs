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

    fn byte_len(&self) -> Option<u64> {
        None
    }
}

impl Read for OpusPassthroughSource {
    fn read(&mut self, mut buf: &mut [u8]) -> std::io::Result<usize> {
        let mut written_len = 0;

        while !buf.is_empty() {
            match &mut self.state {
                State::Header => {
                    // Can't write the header if size_of::<u16>() are free.
                    if buf.len() < std::mem::size_of::<u16>() {
                        return Ok(written_len);
                    }

                    // Read the next valid packet for this track.
                    let packet = self.next_packet()?;

                    // Write the packet header (packet len as a u16).
                    let header_bytes = (packet.data.len() as u16).to_ne_bytes();
                    let transfer_len = header_bytes.len();
                    buf[..transfer_len].copy_from_slice(&header_bytes);

                    // Offset the buffer so the data isn't overwritten.
                    buf = &mut buf[transfer_len..];
                    written_len += transfer_len;

                    // Continue with the frame data itself.
                    self.state = State::Frame { packet, offset: 0 };
                }
                State::Frame { packet, offset } => {
                    // Copy from the remaining data in the packet
                    let remaining_data = &packet.data[*offset..];
                    let transfer_len = buf.len().min(remaining_data.len());
                    buf[..transfer_len].copy_from_slice(&remaining_data[..transfer_len]);

                    // Offset the src so the same data isn't read next time.
                    *offset += transfer_len;

                    // Offset the buffer so the data isn't overwritten.
                    buf = &mut buf[transfer_len..];
                    written_len += transfer_len;

                    // If this was the end of the packet, send the header next.
                    if *offset == packet.data.len() {
                        self.state = State::Header;
                    }
                }
            }
        }

        Ok(written_len)
    }
}

impl Seek for OpusPassthroughSource {
    fn seek(&mut self, _pos: SeekFrom) -> io::Result<u64> {
        // No!
        panic!("Attempting to seek on non-seekable streaming source")
    }
}
