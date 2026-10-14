//! Scripted references and `modify:` scripts, and the build refusal when the
//! script engine is compiled out.

use vantage_vista::VistaFactory;
#[cfg(feature = "rhai")]
use vantage_vista::{Capability, SortDirection};

#[cfg(feature = "rhai")]
use super::{hill_valley, registry_resolver};
use super::{parse, test_db};
use crate::vista::SurrealVistaFactory;

#[cfg(feature = "rhai")]
#[test]
fn scripted_reference_builds_target_via_rhai() {
    let bakery_yaml = r#"
name: bakery
columns:
  id: { type: thing, flags: [id] }
references:
  products:
    table: product
    kind: has_many
    foreign_key: bakery
    surreal:
      rhai: |
        table("product").where("bakery", row.id)
"#;
    let product_yaml = r#"
name: product
columns:
  id: { type: thing, flags: [id] }
  name: { type: string, flags: [title] }
  price: { type: int }
  bakery: { type: thing }
"#;
    let bakery_spec = parse(bakery_yaml);
    let product_spec = parse(product_yaml);
    let resolver = registry_resolver(vec![
        ("bakery".into(), bakery_spec.clone()),
        ("product".into(), product_spec),
    ]);

    let factory = SurrealVistaFactory::new(test_db()).with_resolver(resolver);
    let bakery = factory.build_from_spec(bakery_spec).expect("build bakery");

    // The build_script rode through the extras slot onto the Reference.
    let reference = bakery.reference("products").expect("products ref");
    assert!(reference.build_script.is_some());

    // Traversal evaluates the script: `table("product")` resolves a fresh
    // product Vista (proving the script path fired, not the FK path).
    let child = bakery
        .get_ref("products", Some(&hill_valley()))
        .expect("scripted traverse products");

    let mut cols: Vec<&str> = child.columns().keys().map(String::as_str).collect();
    cols.sort();
    assert_eq!(cols, vec!["bakery", "id", "name", "price"]);
    assert_eq!(child.name(), "product");
}

/// The metadata names a reference's target by the spec it resolves
/// (`product`), not the SurrealDB table that spec reads (`products`), and
/// carries the reference's script.
#[cfg(feature = "rhai")]
#[test]
fn scripted_reference_targets_the_spec_name() {
    let script = r#"table("product").where("bakery", row.id)"#;
    let bakery_spec = parse(&format!(
        r#"
name: bakery
columns:
  id: {{ type: thing, flags: [id] }}
references:
  products:
    table: product
    kind: has_many
    foreign_key: bakery
    surreal:
      rhai: '{script}'
"#
    ));
    let product_spec = parse(
        r#"
name: product
columns:
  id: { type: thing, flags: [id] }
  bakery: { type: thing }
surreal:
  table: products
"#,
    );
    let resolver = registry_resolver(vec![("product".into(), product_spec)]);
    let factory = SurrealVistaFactory::new(test_db()).with_resolver(resolver);
    let bakery = factory.build_from_spec(bakery_spec).expect("build bakery");

    let reference = bakery.reference("products").expect("products ref");
    assert_eq!(reference.target, "product");
    assert_eq!(reference.build_script.as_deref(), Some(script));
}

#[cfg(feature = "rhai")]
#[test]
fn scripted_reference_refuses_vendor_condition_on_table() {
    // `with_condition` needs a Vista in hand (`self`); on a handle from
    // `table(name)` it is an error naming the verb.
    let bakery_yaml = r#"
name: bakery
columns:
  id: { type: thing, flags: [id] }
references:
  products:
    table: product
    kind: has_many
    foreign_key: bakery
    surreal:
      rhai: |
        table("product").with_condition(surreal::ident("bakery") == row.id)
"#;
    let product_yaml = r#"
name: product
columns:
  id: { type: thing, flags: [id] }
  name: { type: string, flags: [title] }
  bakery: { type: thing }
"#;
    let bakery_spec = parse(bakery_yaml);
    let resolver = registry_resolver(vec![
        ("bakery".into(), bakery_spec.clone()),
        ("product".into(), parse(product_yaml)),
    ]);
    let factory = SurrealVistaFactory::new(test_db()).with_resolver(resolver);
    let bakery = factory.build_from_spec(bakery_spec).expect("build bakery");

    let err = bakery
        .get_ref("products", Some(&hill_valley()))
        .map(drop)
        .expect_err("with_condition on table(...) must fail");
    assert!(
        err.to_string().contains(r#"verb: "with_condition""#),
        "{err}"
    );
}

#[cfg(feature = "rhai")]
#[test]
fn modify_script_tweaks_built_vista_with_vendor_condition() {
    // The YAML builds a normal, writable `client` table; the `modify` script
    // then narrows it with an expression condition YAML can't express,
    // routed through `add_native_condition` under its rendered SurrealQL.
    let yaml = r#"
name: client
columns:
  id: { type: thing, flags: [id] }
  name: { type: string, flags: [title] }
  is_paying_client: { type: bool }
surreal:
  table: clients
  modify: |
    self.with_condition(surreal::ident("is_paying_client") == true)
       .sort("name", "asc")
"#;
    let spec = parse(yaml);
    let factory = SurrealVistaFactory::new(test_db());
    let vista = factory.build_from_spec(spec).expect("build + modify");

    // The base table stays writable (a condition narrows, it doesn't make
    // the vista read-only), and the schema is intact.
    assert!(vista.capabilities().has(Capability::Insert));
    let mut cols: Vec<&str> = vista.columns().keys().map(String::as_str).collect();
    cols.sort();
    assert_eq!(cols, vec!["id", "is_paying_client", "name"]);

    assert_eq!(vista.query().native, vec!["is_paying_client = true"]);
    assert_eq!(
        vista.query().order,
        Some(("name".to_string(), SortDirection::Ascending))
    );
    let preview = vista.preview().query.expect("preview query");
    assert!(
        preview.contains("WHERE is_paying_client = true"),
        "{preview}"
    );
}

/// Without the script engine, a spec that carries a script fails to build,
/// naming the vista (and the relation, for a reference script), instead of
/// silently dropping the script.
#[cfg(not(feature = "rhai"))]
#[test]
fn scripts_without_rhai_fail_the_build() {
    let context = |err: &vantage_core::VantageError, key: &str| {
        err.context
            .get(key)
            .map(|v| v.trim_matches('"').to_string())
    };
    let with_ref_script = parse(
        r#"
name: bakery
columns:
  id: { type: thing, flags: [id] }
references:
  products:
    table: product
    kind: has_many
    foreign_key: bakery
    surreal:
      rhai: 'table("product")'
"#,
    );
    let err = SurrealVistaFactory::new(test_db())
        .build_from_spec(with_ref_script)
        .unwrap_err();
    assert_eq!(context(&err, "vista").as_deref(), Some("bakery"), "{err:?}");
    assert_eq!(
        context(&err, "relation").as_deref(),
        Some("products"),
        "{err:?}"
    );

    let with_modify = parse(
        r#"
name: bakery
columns:
  id: { type: thing, flags: [id] }
surreal:
  modify: 'self'
"#,
    );
    let err = SurrealVistaFactory::new(test_db())
        .build_from_spec(with_modify)
        .unwrap_err();
    assert_eq!(context(&err, "vista").as_deref(), Some("bakery"), "{err:?}");
}
