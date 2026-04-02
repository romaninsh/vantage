mod common;

use common::{connect_surrealdb, setup_cache, Bakery, Client};
use vantage_dataset::dataset::{
    InsertableDataSet, ReadableAsDataSet, ReadableDataSet, ReadableValueSet, WritableDataSet,
};
use vantage_live::prelude::*;
use vantage_table::Table;

#[tokio::test]
#[ignore]
async fn test_readable_dataset_polymorphism() {
    // TODO: implement test case for using LiveTable as ReadableDataSet trait object
    // - Create LiveTable
    // - Cast to &dyn ReadableDataSet<Client>
    // - Call get(), get_id(), get_some()
    // - Verify works through trait object (polymorphism)
}

#[tokio::test]
#[ignore]
async fn test_writable_dataset_replace() {
    // TODO: implement test case for WritableDataSet::replace_id
    // - Create LiveTable
    // - Call replace_id() to update existing client
    // - Verify both cache and backend updated
    // - Should work without going through RecordEdit
}

#[tokio::test]
#[ignore]
async fn test_writable_dataset_update_callback() {
    // TODO: implement test case for WritableDataSet::update with callback
    // - Create LiveTable with multiple clients
    // - Call update(|client| { client.is_paying_client = true; })
    // - Verify all clients updated in both cache and backend
}

#[tokio::test]
#[ignore]
async fn test_insertable_dataset_insert() {
    // TODO: implement test case for InsertableDataSet::insert
    // - Create LiveTable
    // - Call insert(new_client)
    // - Should return Some(id) with generated ID
    // - Verify inserted to both cache and backend
    // - Compare with new_record() + save() workflow
}

#[tokio::test]
#[ignore]
async fn test_readable_value_set_json() {
    // TODO: implement test case for ReadableValueSet returning JSON
    // - Create LiveTable
    // - Call get_values() to get Vec<serde_json::Value>
    // - Verify JSON structure matches entity fields
    // - Useful for generic UI grids that don't know entity type
}

#[tokio::test]
#[ignore]
async fn test_readable_value_set_get_id_value() {
    // TODO: implement test case for getting single record as JSON value
    // - Create LiveTable
    // - Call get_id_value(id)
    // - Returns serde_json::Value for that record
    // - Can inspect fields dynamically
}

#[tokio::test]
#[ignore]
async fn test_readable_value_set_get_some_value() {
    // TODO: implement test case for get_some_value
    // - Create LiveTable
    // - Call get_some_value()
    // - Returns Option<serde_json::Value> with first record
    // - Useful for "get any record" scenarios
}

#[tokio::test]
#[ignore]
async fn test_readable_as_dataset_type_conversion() {
    // TODO: implement test case for ReadableAsDataSet trait
    // - Create LiveTable<Client>
    // - Call get_as::<DifferentEntity>()
    // - Should deserialize client data into different type
    // - Useful for view models or DTOs
}

#[tokio::test]
#[ignore]
async fn test_drop_in_replacement_for_table() {
    // TODO: implement test case showing LiveTable can replace Table
    // - Write generic function accepting impl ReadableDataSet<Client>
    // - Pass regular Table - works
    // - Pass LiveTable - also works
    // - No code changes needed in consuming function
}

#[tokio::test]
#[ignore]
async fn test_dataset_trait_with_generic_function() {
    // TODO: implement test case for generic function accepting dataset traits
    // - Define fn count_records<D: ReadableDataSet<E>, E>
    // - Call with both Table and LiveTable
    // - Both should work identically
    // - Demonstrates trait abstraction works
}

#[tokio::test]
#[ignore]
async fn test_writable_dataset_delete_operations() {
    // TODO: implement test case for delete operations through WritableDataSet
    // - Create LiveTable with test data
    // - Call delete_id() to remove specific record
    // - Verify removed from both cache and backend
    // - Call delete_all() to clear everything
    // - Verify both storages empty
}

#[tokio::test]
#[ignore]
async fn test_readable_dataset_empty_table() {
    // TODO: implement test case for empty dataset behavior
    // - Create LiveTable for table with no records
    // - get() returns empty Vec
    // - get_some() returns None
    // - get_values() returns empty Vec
    // - No errors, just empty results
}

#[tokio::test]
#[ignore]
async fn test_insertable_dataset_batch_insert() {
    // TODO: implement test case for multiple inserts
    // - Create LiveTable
    // - Insert multiple records via insert()
    // - Verify all persisted with generated IDs
    // - Check cache and backend consistency
}

#[tokio::test]
#[ignore]
async fn test_trait_object_storage() {
    // TODO: implement test case for storing LiveTable as trait object
    // - Create Box<dyn ReadableDataSet<Client>>
    // - Store LiveTable in it
    // - Use through trait object interface
    // - Demonstrates dynamic dispatch works
}

#[tokio::test]
#[ignore]
async fn test_writable_value_set_patch() {
    // TODO: implement test case for patch_id partial update
    // - Create LiveTable
    // - Call patch_id(id, json!({ "name": "Updated" }))
    // - Should update only specified fields
    // - Other fields unchanged
    // - Works with both cache and backend
}

#[tokio::test]
#[ignore]
async fn test_all_traits_implemented() {
    // TODO: implement test case verifying all expected traits available
    // - Create LiveTable instance
    // - Verify can cast to each trait:
    //   - ReadableDataSet
    //   - ReadableValueSet
    //   - ReadableAsDataSet
    //   - WritableDataSet
    //   - WritableValueSet
    //   - InsertableDataSet
    // - Demonstrates complete trait coverage
}
