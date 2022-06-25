mod brain;
mod copy_buffered;
mod error;
mod input;
mod ring_buffer;
mod song;
mod speaker;

pub use self::brain::*;
pub use self::error::*;
pub use self::song::*;
pub use self::speaker::*;

lazy_static::lazy_static! {
    static ref HTTP_CLIENT: reqwest::Client = reqwest::Client::new();
}
