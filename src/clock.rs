//! The zone snapshots are stamped in: `[chronometer] timezone` from the configuration, else the local clock.

use jiff::tz::TimeZone;

/// Read the configured zone; anything missing or unreadable falls back to `local` without a word.
pub fn zone(config_text: Option<&str>, local: TimeZone) -> TimeZone {
    config_text
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|table| table.get("chronometer")?.get("timezone")?.as_str().map(str::to_owned))
        .and_then(|name| TimeZone::get(&name).ok())
        .unwrap_or(local)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKYO: &str = "[chronometer]\ntimezone = \"Asia/Tokyo\"\n";

    #[test]
    fn a_configured_zone_is_read() {
        assert_eq!(zone(Some(TOKYO), TimeZone::UTC).iana_name(), Some("Asia/Tokyo"));
    }

    #[test]
    fn without_a_readable_zone_the_local_clock_stamps() {
        let local = TimeZone::fixed(jiff::tz::offset(3));
        let texts = [
            None,
            Some("[chronometer]\ntimezone = \"Atlantis/Capital\"\n"),
            Some("not toml at all"),
            Some("[chronometer]\n"),
        ];
        for text in texts {
            assert_eq!(zone(text, local.clone()), local, "{text:?}");
        }
    }
}
