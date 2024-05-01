use crate::PlayConfig;
use std::fmt::{Debug, Display, Formatter};
use std::io::{Error, ErrorKind, Result};
use std::process::ExitStatus;
use tokio::process::Command;

#[derive(Debug)]
pub struct StatusCodeError(ExitStatus);

impl Display for StatusCodeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "status code {}", self.0)
    }
}

impl std::error::Error for StatusCodeError {}

pub async fn get_ytdl_version(config: &PlayConfig<'_>) -> Result<String> {
    let ytdl = Command::new(config.ytdl_name)
        .arg("--version")
        .output()
        .await?;

    if ytdl.status.success() {
        match String::from_utf8(ytdl.stdout) {
            Ok(mut version_raw) => {
                // remove any trailing whitespace (probably a newline)
                version_raw.truncate(version_raw.trim_end().len());
                Ok(version_raw)
            }
            Err(err) => Err(Error::new(ErrorKind::Other, err)),
        }
    } else {
        Err(Error::new(ErrorKind::Other, StatusCodeError(ytdl.status)))
    }
}
