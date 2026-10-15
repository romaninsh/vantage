//! Joined values on read: one and two hops, a missing target, filters, order,
//! keyset paging, search, aggregates and live updates.

use bson::doc;
use ciborium::Value as CborValue;
use futures_util::StreamExt;
use vantage_dataset::prelude::*;
use vantage_mongodb::{AnyMongoType, MongoDB, MongoId};
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::{Capability, FilterOp, SortDirection};

use super::{ORDER, TestResult, column, orders, pair, resolver, seed, setup, spec, teardown, text};

#[tokio::test]
async fn dotted_column_reads_joined_value() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let rows = orders(&db).list_values().await?;
    assert_eq!(
        column(&rows, "client.name"),
        vec![
            pair("o1", Some("Marty")),
            pair("o2", Some("Marty")),
            pair("o3", Some("Doc")),
            pair("o4", None),
        ]
    );
    // The foreign key itself still reads, and no join scaffolding leaks.
    let o1 = &rows["o1"];
    assert!(text(o1, "client_id").is_some());
    assert!(o1.keys().all(|k| !k.starts_with("__")), "{o1:?}");

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn typed_table_joins_through_a_target_that_names_no_id_column() -> TestResult {
    let (db, name) = setup().await;
    let marty = bson::oid::ObjectId::new();
    db.doc_collection("clients")
        .insert_one(doc! { "_id": marty, "name": "Marty" })
        .await?;
    db.doc_collection("orders")
        .insert_many(vec![
            doc! { "_id": "o1", "client_id": marty, "item": "skateboard" },
            doc! { "_id": "o2", "client_id": bson::oid::ObjectId::new(), "item": "almanac" },
        ])
        .await?;

    let mut orders = Table::<MongoDB, EmptyEntity>::new("orders", db.clone())
        .with_id_column("_id")
        .with_column_of::<AnyMongoType>("client_id")
        .with_column_of::<String>("item")
        .with_one("client", "client_id", |db| {
            Table::<MongoDB, EmptyEntity>::new("clients", db).with_column_of::<String>("name")
        });
    orders.add_imported_column("client.name")?;

    let rows = orders.list_values().await?;
    let joined = |id: &str| {
        rows[&MongoId::from(id)]
            .get("client.name")
            .and_then(|v| v.try_get::<String>())
    };
    assert_eq!(joined("o1").as_deref(), Some("Marty"));
    assert_eq!(joined("o2"), None);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn a_string_foreign_key_does_not_join_an_object_id_key() -> TestResult {
    let (db, name) = setup().await;
    let marty = bson::oid::ObjectId::new();
    db.doc_collection("clients")
        .insert_one(doc! { "_id": marty, "name": "Marty" })
        .await?;
    db.doc_collection("orders")
        .insert_many(vec![
            doc! { "_id": "typed", "client_id": marty, "item": "skateboard" },
            doc! { "_id": "text", "client_id": marty.to_hex(), "item": "almanac" },
        ])
        .await?;

    // $lookup compares types strictly: the hex text joins nothing.
    let rows = orders(&db).list_values().await?;
    assert_eq!(
        column(&rows, "client.name"),
        vec![pair("typed", Some("Marty")), pair("text", None)]
    );
    assert_eq!(rows.len(), 2);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn dotted_column_two_hops() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let rows = orders(&db).list_values().await?;
    assert_eq!(
        column(&rows, "client.country.name"),
        vec![
            pair("o1", Some("USA")),
            pair("o2", Some("USA")),
            pair("o3", Some("Canada")),
            pair("o4", None),
        ]
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn dotted_column_missing_target_is_null() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let vista = orders(&db);
    assert_eq!(vista.count().await?, 4);
    let o4 = vista
        .get_value("o4")
        .await?
        .expect("dangling order still reads");
    assert_eq!(text(&o4, "item").as_deref(), Some("hoverboard"));
    assert_eq!(text(&o4, "client.name"), None);
    assert_eq!(text(&o4, "client.country.name"), None);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn dotted_column_filter_and_order() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let mut marty = orders(&db);
    marty.add_condition("client.name", FilterOp::Eq, CborValue::Text("Marty".into()))?;
    let rows = marty.list_values().await?;
    let mut ids: Vec<&str> = rows.keys().map(String::as_str).collect();
    ids.sort();
    assert_eq!(ids, vec!["o1", "o2"]);
    assert_eq!(marty.count().await?, 2);

    let mut by_country = orders(&db);
    by_country.add_condition(
        "client.country.name",
        FilterOp::Eq,
        CborValue::Text("Canada".into()),
    )?;
    let rows = by_country.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), vec!["o3"]);

    let mut sorted = orders(&db);
    sorted.set_order(Some(("client.name", SortDirection::Descending)))?;
    let rows = sorted.list_values().await?;
    let names: Vec<Option<String>> = rows.values().map(|r| text(r, "client.name")).collect();
    assert_eq!(
        names,
        vec![
            Some("Marty".into()),
            Some("Marty".into()),
            Some("Doc".into()),
            None,
        ]
    );

    // Keyset paging seeks on the joined sort key.
    let mut paged = orders(&db);
    paged.set_order(Some(("client.name", SortDirection::Ascending)))?;
    let (first, token) = paged.fetch_cursor(None, 3).await?;
    let (second, token) = paged.fetch_cursor(token, 3).await?;
    assert!(token.is_none());
    let seen: Vec<&str> = first
        .rows
        .iter()
        .chain(&second.rows)
        .map(|(id, _)| id.as_str())
        .collect();
    // A missing join sorts first; equal names fall back to `_id`.
    assert_eq!(seen, vec!["o4", "o3", "o1", "o2"]);

    // Quicksearch covers a searchable joined column.
    let mut searched = orders(&db);
    searched.set_search(Some("doc"))?;
    let rows = searched.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), vec!["o3"]);
    assert_eq!(searched.count().await?, 1);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn a_filter_value_that_looks_like_a_field_reference_is_matched_as_plain_text() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;
    db.doc_collection("orders")
        .insert_many(vec![
            doc! { "_id": "d1", "client_id": bson::oid::ObjectId::new(), "item": "$__vc0" },
            doc! { "_id": "d2", "client_id": bson::oid::ObjectId::new(), "item": "$__vcx" },
        ])
        .await?;

    for (literal, id) in [("$__vc0", "d1"), ("$__vcx", "d2")] {
        let mut v = orders(&db);
        v.add_condition("item", FilterOp::Eq, CborValue::Text(literal.into()))?;
        let rows = v.list_values().await?;
        assert_eq!(rows.keys().collect::<Vec<_>>(), vec![id], "{literal}");
    }

    // A number is data too: nothing here is stored as one.
    let mut numeric = orders(&db);
    numeric.add_condition("item", FilterOp::Eq, CborValue::Integer(5.into()))?;
    assert_eq!(numeric.count().await?, 0);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn preview_of_a_vista_filtered_on_a_joined_column_shows_the_lookup() -> TestResult {
    let (db, name) = setup().await;

    let mut filtered = orders(&db);
    filtered.add_condition("item", FilterOp::Eq, CborValue::Text("almanac".into()))?;
    filtered.add_condition("client.name", FilterOp::Eq, CborValue::Text("Marty".into()))?;
    let query = filtered.preview().query.expect("a rendered query");
    assert!(query.contains("$lookup"), "{query}");

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn dotted_column_aggregate_is_refused() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let table = db
        .vista_factory()
        .with_resolver(resolver())
        .table_from_spec(&spec(ORDER))?;
    let joined = table
        .get_column::<AnyMongoType>("client.name")
        .expect("joined column");
    for result in [
        table.get_sum(&joined).await,
        table.get_max(&joined).await,
        table.get_min(&joined).await,
    ] {
        let err = result.expect_err("aggregating a join is unsupported");
        assert!(err.is_unsupported(), "{err}");
    }
    // A stored column still aggregates.
    let item = table.get_column::<AnyMongoType>("item").expect("item");
    assert_eq!(
        table.get_max(&item).await?.value().clone(),
        bson::Bson::String("skateboard".into())
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn get_value_includes_joined_column() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let vista = orders(&db);
    let o3 = vista.get_value("o3").await?.expect("o3");
    assert_eq!(text(&o3, "client.name").as_deref(), Some("Doc"));
    assert_eq!(text(&o3, "client.country.name").as_deref(), Some("Canada"));

    // Rows re-read for live updates carry the join too. The test deployment
    // is a replica set, so change streams are available.
    assert!(vista.capabilities().has(Capability::Subscribe));
    let mut stream = vista.watch().await?;
    db.doc_collection("orders")
        .update_one(
            doc! { "_id": "o3" },
            doc! { "$set": { "item": "delorean" } },
        )
        .await?;
    match stream.next().await.expect("an event")? {
        vantage_vista::VistaChange::Updated { id, value } => {
            assert_eq!(id, "o3");
            assert_eq!(text(&value, "client.name").as_deref(), Some("Doc"));
        }
        other => panic!("unexpected change {other:?}"),
    }

    teardown(&db, &name).await;
    Ok(())
}
