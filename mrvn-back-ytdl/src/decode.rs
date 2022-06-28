use std::io::{Read, Write};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions, ReadOnlySource};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub fn decode<I: Read + Send + Sync + 'static, O: Write>(hint: &Hint, input: I, mut output: O) {
    let probe = symphonia::default::get_probe();

    let source = ReadOnlySource::new(input);
    let stream = MediaSourceStream::new(Box::new(source), MediaSourceStreamOptions::default());
    let probe_result = probe
        .format(
            hint,
            stream,
            &FormatOptions {
                prebuild_seek_index: false,
                seek_index_fill_rate: 0,
                enable_gapless: false,
            },
            &MetadataOptions::default(),
        )
        .unwrap();

    let default_track = probe_result.format.default_track().unwrap();
    log::info!("Decoded track = {:#?}", default_track);
}
