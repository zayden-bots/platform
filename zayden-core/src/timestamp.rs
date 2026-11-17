use serenity::all::Timestamp as SerenityTimestamp;
use serenity::model::timestamp::InvalidTimestamp;

pub fn serenity_to_jiff(
    ts: &SerenityTimestamp,
) -> Result<jiff::Timestamp, jiff::Error> {
    jiff::Timestamp::from_nanosecond(ts.as_nanosecond())
}

// Discord timestamps carry millisecond precision.
pub fn jiff_to_serenity(
    ts: jiff::Timestamp,
) -> Result<SerenityTimestamp, InvalidTimestamp> {
    SerenityTimestamp::from_millis(ts.as_millisecond())
}
