use serenity::all::Timestamp as SerenityTimestamp;
use zayden_core::{jiff_to_serenity, serenity_to_jiff};

#[test]
fn serenity_to_jiff_is_exact() {
    let ts = SerenityTimestamp::from_millis(1_750_000_000_123).unwrap();

    assert_eq!(
        serenity_to_jiff(&ts).unwrap(),
        jiff::Timestamp::from_millisecond(1_750_000_000_123).unwrap()
    );
}

#[test]
fn jiff_to_serenity_round_trips_milliseconds() {
    let ts = jiff::Timestamp::from_millisecond(1_750_000_000_123).unwrap();

    assert_eq!(
        jiff_to_serenity(ts).unwrap().unix_timestamp_millis(),
        1_750_000_000_123
    );
}

#[test]
fn jiff_to_serenity_truncates_below_a_millisecond() {
    let ts = jiff::Timestamp::new(1_750_000_000, 123_456_789).unwrap();

    assert_eq!(
        jiff_to_serenity(ts).unwrap().unix_timestamp_millis(),
        1_750_000_000_123
    );
}
