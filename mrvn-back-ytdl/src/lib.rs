mod brain;
mod error;
mod formats;
mod input;
mod song;
mod source;
mod speaker;

pub use self::brain::*;
pub use self::error::*;
pub use self::song::*;
pub use self::speaker::*;

lazy_static::lazy_static! {
    static ref HTTP_CLIENT: reqwest::Client = reqwest::Client::new();
}
