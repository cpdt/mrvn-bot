use crate::formats::MpegTsReader;
use lazy_static::lazy_static;
use songbird::{Config, Songbird};
use std::ops::Deref;
use std::sync::Arc;
use symphonia::core::probe::Probe;
use symphonia::default::register_enabled_formats;

lazy_static! {
    static ref PROBE: Probe = {
        let mut probe = Probe::default();
        register_enabled_formats(&mut probe);
        probe.register_all::<MpegTsReader>();
        probe
    };
}

pub fn songbird() -> Arc<Songbird> {
    Songbird::serenity_from_config(Config::default().format_registry(PROBE.deref()))
}
