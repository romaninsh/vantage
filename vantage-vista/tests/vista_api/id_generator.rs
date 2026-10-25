use vantage_core::error;
use vantage_vista::{IdGenerator, ShortId};

fn uuid_version(id: &str) -> usize {
    uuid::Uuid::parse_str(id).unwrap().get_version_num()
}

#[test]
fn short_ids_have_the_length_and_alphabet_asked_for() {
    let short = ShortId::new(10, ShortId::BASE58).unwrap();
    assert_eq!(short.length(), 10);
    assert_eq!(short.alphabet(), ShortId::BASE58);
    for _ in 0..200 {
        let id = short.generate();
        assert_eq!(id.chars().count(), 10, "{id}");
        assert!(id.chars().all(|c| ShortId::BASE58.contains(c)), "{id}");
    }
}

#[test]
fn short_ids_reach_every_character_of_a_small_alphabet() {
    let id = ShortId::new(64, "ab").unwrap().generate();
    assert!(id.contains('a') && id.contains('b'), "{id}");
}

#[test]
fn base58_has_58_characters_without_lookalikes() {
    assert_eq!(ShortId::BASE58.chars().count(), 58);
    assert!(!ShortId::BASE58.contains(&['0', 'O', 'I', 'l'][..]));
    assert_eq!(ShortId::ALNUM.chars().count(), 62);
}

#[test]
fn short_id_refuses_bad_lengths_and_alphabets() {
    for (length, alphabet) in [
        (0, ShortId::BASE58),
        (ShortId::MAX_LENGTH + 1, ShortId::BASE58),
        (8, "a"),
        (8, "abca"),
        (8, "ab c"),
        (8, "ab\u{7}"),
    ] {
        assert!(
            ShortId::new(length, alphabet).is_err(),
            "{length} over {alphabet:?} accepted"
        );
    }
    assert!(ShortId::new(ShortId::MAX_LENGTH, ShortId::ALNUM).is_ok());
    assert!(IdGenerator::short(0, ShortId::BASE58).is_err());
}

#[test]
fn every_generator_mints_in_its_own_format() {
    assert_eq!(uuid_version(&IdGenerator::default().generate()), 7);
    assert_eq!(
        uuid_version(&IdGenerator::UuidV7.try_generate().unwrap()),
        7
    );
    assert_eq!(uuid_version(&IdGenerator::UuidV4.generate()), 4);

    let short = IdGenerator::short(6, "xy").unwrap().generate();
    assert_eq!(short.len(), 6, "{short}");
    assert!(short.chars().all(|c| c == 'x' || c == 'y'), "{short}");

    let custom = IdGenerator::custom(|| "c".into());
    assert_eq!(custom.generate(), "c");
    assert_eq!(custom.try_generate().unwrap(), "c");
}

#[test]
fn a_fallible_generator_reports_its_error_through_try_generate() {
    let failing = IdGenerator::fallible(|| Err(error!("no ids today")));
    let err = failing.try_generate().unwrap_err();
    assert!(err.to_string().contains("no ids today"), "{err}");

    let ok = IdGenerator::fallible(|| Ok("made".into()));
    assert_eq!(ok.try_generate().unwrap(), "made");
    assert_eq!(ok.generate(), "made");
}

#[test]
#[should_panic(expected = "id generator failed")]
fn generate_panics_when_a_fallible_generator_fails() {
    IdGenerator::fallible(|| Err(error!("no ids today"))).generate();
}

#[test]
fn debug_names_the_generator_and_leaves_closures_out() {
    assert_eq!(format!("{:?}", IdGenerator::UuidV7), "IdGenerator::UuidV7");
    assert_eq!(format!("{:?}", IdGenerator::UuidV4), "IdGenerator::UuidV4");
    assert_eq!(
        format!("{:?}", IdGenerator::custom(|| "c".into())),
        "IdGenerator::Custom"
    );
    assert_eq!(
        format!("{:?}", IdGenerator::fallible(|| Ok("f".into()))),
        "IdGenerator::Fallible"
    );
    let short = format!("{:?}", IdGenerator::short(4, "ab").unwrap());
    assert!(short.starts_with("IdGenerator::Short(ShortId"), "{short}");
    assert!(
        short.contains("length: 4") && short.contains("\"ab\""),
        "{short}"
    );
}
