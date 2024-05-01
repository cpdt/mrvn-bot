use crate::config::Config;

pub const MAX_COLUMNS: usize = 54;
pub const BEFORE_PROGRESS_BAR: &str = " [";
pub const AFTER_PROGRESS_BAR: &str = "]";

pub fn format_time(config: &Config, time_seconds: f64, duration_seconds: Option<f64>) -> String {
    match duration_seconds {
        Some(duration) => {
            let (formatted_duration, minutes_width) = config.format_time(duration, 0);
            let (formatted_time, _) = config.format_time(time_seconds, minutes_width);

            config.get_message(
                "time_and_duration",
                &[("time", &formatted_time), ("duration", &formatted_duration)],
            )
        }
        None => {
            let (formatted_time, _) = config.format_time(time_seconds, 0);

            config.get_message(
                "time_and_duration",
                &[
                    ("time", &formatted_time),
                    ("duration", config.get_raw_message("duration.unknown")),
                ],
            )
        }
    }
}

pub fn format_time_bar(
    config: &Config,
    time_seconds: f64,
    duration_seconds: Option<f64>,
) -> String {
    let time = format_time(config, time_seconds, duration_seconds);
    let progress_str = match duration_seconds {
        Some(duration) => {
            let width =
                (MAX_COLUMNS - time.len() - BEFORE_PROGRESS_BAR.len() - AFTER_PROGRESS_BAR.len())
                    .max(1);
            let progress = (time_seconds / duration).clamp(0., 1.);
            let progress_width = (width as f64 * progress) as usize;

            format!(
                "{}{:=<width$}{:->inv_width$}{}",
                BEFORE_PROGRESS_BAR,
                "",
                "",
                AFTER_PROGRESS_BAR,
                width = progress_width,
                inv_width = width - progress_width
            )
        }
        None => "".to_string(),
    };

    format!("{}{}", time, progress_str)
}
