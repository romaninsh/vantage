use ciborium::Value as CborValue;
use vantage_dataset::traits::ReadableValueSet;
use vantage_vista::mocks::MockShell;
use vantage_vista::{
    Column, ColumnFlag, FilterOp, JoinKey, Reference, ReferenceKind, Vista, VistaMetadata,
};

use super::{i, rec, t};

fn orders() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
        .with_column(Column::new("client_id", "String"))
        .with_column(Column::new("region", "String"))
        .with_column(Column::new("dims", "map"))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_record(
            "o1",
            rec(&[("id", t("o1")), ("client_id", t("c1")), ("region", t("eu"))]),
        )
        .with_record(
            "o2",
            rec(&[("id", t("o2")), ("client_id", t("c2")), ("region", t("eu"))]),
        )
        .with_record(
            "o3",
            rec(&[("id", t("o3")), ("client_id", t("c1")), ("region", t("us"))]),
        );
    Vista::new("order", shell)
}

fn has_many_orders() -> Reference {
    Reference::has_many("orders", "order", JoinKey::new("id", "client_id"))
}

async fn ids(vista: &Vista) -> Vec<String> {
    vista.list_values().await.unwrap().into_keys().collect()
}

fn described(err: &vantage_core::VantageError) -> String {
    format!("{err:?} {err}")
}

#[tokio::test]
async fn narrowing_a_join_constrains_the_target_by_the_parent_value() {
    let narrowed = has_many_orders()
        .narrow(orders(), &rec(&[("id", t("c1"))]))
        .unwrap();
    assert!(narrowed.is_narrowed());
    assert_eq!(narrowed.query().conditions.len(), 1);
    assert_eq!(narrowed.query().conditions[0].column, "client_id");
    assert_eq!(narrowed.query().conditions[0].op, FilterOp::Eq);
    assert_eq!(ids(&narrowed).await, ["o1", "o3"]);
}

#[tokio::test]
async fn every_key_of_a_join_narrows() {
    let reference: Reference = serde_yaml_ng::from_str(
        "name: orders\nkind: has_many\ntarget: order\n\
         keys: [{from: id, to: client_id}, {from: region, to: region}]\n",
    )
    .unwrap();
    let narrowed = reference
        .narrow(orders(), &rec(&[("id", t("c1")), ("region", t("eu"))]))
        .unwrap();
    assert_eq!(narrowed.query().conditions.len(), 2);
    assert_eq!(ids(&narrowed).await, ["o1"]);
}

#[tokio::test]
async fn a_join_without_a_to_column_constrains_the_targets_id() {
    let reference = Reference::has_one("order", "order", JoinKey::to_target_id("order_id"));
    let narrowed = reference
        .narrow(orders(), &rec(&[("order_id", t("o2"))]))
        .unwrap();
    assert_eq!(narrowed.query().conditions[0].column, "id");
    assert_eq!(ids(&narrowed).await, ["o2"]);
    assert_eq!(
        JoinKey::to_target_id("order_id")
            .target_column(&orders())
            .unwrap(),
        "id"
    );
    assert_eq!(
        JoinKey::new("a", "client_id")
            .target_column(&orders())
            .unwrap(),
        "client_id"
    );
}

#[test]
fn a_target_without_an_id_column_is_an_error_naming_the_relation() {
    let target = Vista::new("bare", MockShell::new().with_metadata(VistaMetadata::new()));
    let reference = Reference::has_one("owner", "bare", JoinKey::to_target_id("owner_id"));
    let err = reference
        .narrow(target, &rec(&[("owner_id", t("x"))]))
        .unwrap_err();
    assert!(described(&err).contains("owner"), "{}", described(&err));
}

#[test]
fn a_contained_relation_or_a_keyless_join_has_nothing_to_narrow_by() {
    let contained = Reference::contains_many("lines", "lines");
    let err = contained.narrow(orders(), &rec(&[])).unwrap_err();
    assert!(err.is_incorrect_usage(), "{err:?}");

    let mut keyless = has_many_orders();
    keyless.keys.clear();
    let err = keyless.narrow(orders(), &rec(&[])).unwrap_err();
    assert!(err.is_incorrect_usage(), "{err:?}");
}

#[tokio::test]
async fn a_dotted_join_column_reads_the_nested_value() {
    let reference = Reference::has_many("orders", "order", JoinKey::new("owner.name", "client_id"));
    let parent = rec(&[("owner", CborValue::Map(vec![(t("name"), t("c2"))]))]);
    let narrowed = reference.narrow(orders(), &parent).unwrap();
    assert_eq!(ids(&narrowed).await, ["o2"]);
}

#[test]
fn a_missing_parent_column_names_the_relation_and_the_column() {
    let err = has_many_orders()
        .narrow(orders(), &rec(&[("other", i(1))]))
        .unwrap_err();
    let text = described(&err);
    assert!(text.contains("orders"), "{text}");
    assert!(text.contains("id"), "{text}");
}

#[test]
fn a_refused_join_column_stays_a_refusal() {
    // `dims.w` is a path the mock refuses rather than evaluates.
    let reference = Reference::has_many("orders", "order", JoinKey::new("id", "dims.w"));
    let err = reference
        .narrow(orders(), &rec(&[("id", t("c1"))]))
        .unwrap_err();
    assert!(err.is_unsupported() || err.is_unimplemented(), "{err:?}");
}

#[test]
fn an_unknown_target_column_is_a_plain_error_naming_the_relation() {
    let reference = Reference::has_many("orders", "order", JoinKey::new("id", "nope"));
    let err = reference
        .narrow(orders(), &rec(&[("id", t("c1"))]))
        .unwrap_err();
    assert!(!err.is_unsupported() && !err.is_unimplemented(), "{err:?}");
    assert!(described(&err).contains("orders"), "{}", described(&err));
}

fn parse(yaml: &str) -> Result<Reference, String> {
    serde_yaml_ng::from_str::<Reference>(yaml).map_err(|e| e.to_string())
}

#[test]
fn references_round_trip_through_yaml() {
    let join = Reference::has_many("orders", "order", JoinKey::new("id", "client_id"))
        .with_build_script("table(\"order\")")
        .unwrap();
    let contained = Reference::contains_many("lines", "lines");
    for reference in [join, contained] {
        let yaml = serde_yaml_ng::to_string(&reference).unwrap();
        assert_eq!(parse(&yaml).unwrap(), reference, "{yaml}");
    }
}

#[test]
fn a_join_needs_a_key_and_takes_no_host_column() {
    let keyless = parse("name: client\nkind: has_one\ntarget: client\n").unwrap_err();
    assert!(keyless.contains("client"), "{keyless}");
    let hosted = parse(
        "name: client\nkind: has_one\ntarget: client\nkeys: [{from: client_id}]\nhost_column: c\n",
    )
    .unwrap_err();
    assert!(hosted.contains("host_column"), "{hosted}");
}

#[test]
fn a_contained_relation_needs_a_host_column_and_takes_no_keys_or_script() {
    let base = "name: lines\nkind: contains_many\ntarget: lines\n";
    assert!(parse(base).unwrap_err().contains("host_column"));
    let keyed = parse(&format!("{base}host_column: lines\nkeys: [{{from: id}}]\n"));
    assert!(keyed.is_err());
    let scripted = parse(&format!("{base}host_column: lines\nbuild_script: x\n")).unwrap_err();
    assert!(scripted.contains("lines"), "{scripted}");
}

#[test]
fn the_builder_refuses_a_build_script_on_a_contained_relation() {
    let err = Reference::contains_one("lines", "lines")
        .with_build_script("x")
        .unwrap_err();
    assert!(
        !err.is_unsupported() && !err.is_incorrect_usage(),
        "{err:?}"
    );
    assert!(described(&err).contains("lines"));
}

#[test]
fn kinds_spell_themselves_as_yaml_does() {
    for kind in [
        ReferenceKind::HasOne,
        ReferenceKind::HasMany,
        ReferenceKind::ContainsOne,
        ReferenceKind::ContainsMany,
    ] {
        let yaml = serde_yaml_ng::to_string(&kind).unwrap();
        assert_eq!(yaml.trim(), kind.as_str());
    }
    assert!(ReferenceKind::ContainsOne.is_contained());
    assert!(ReferenceKind::ContainsMany.is_contained());
    assert!(!ReferenceKind::HasOne.is_contained());
    assert!(!ReferenceKind::HasMany.is_contained());
}
