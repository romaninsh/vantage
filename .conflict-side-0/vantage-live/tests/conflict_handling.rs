mod common;

use common::{connect_surrealdb, setup_cache, Client, CLIENT_DOC};
use vantage_live::prelude::*;
use vantage_table::Table;

#[tokio::test]
#[ignore]
async fn test_refresh_snapshot_no_conflict() {
    // TODO: implement test case for refreshing snapshot when remote changed different fields
    // - Edit client locally (change name)
    // - Backend changes different field (contact_details)
    // - refresh_snapshot() should return empty conflicts list
    // - Local changes preserved, snapshot updated with remote changes
}

#[tokio::test]
#[ignore]
async fn test_refresh_snapshot_with_conflict() {
    // TODO: implement test case for detecting conflict when same field changed locally and remotely
    // - Edit client locally (change email)
    // - Backend also changes email
    // - refresh_snapshot() should return ["email"] in conflicts list
    // - Local change preserved, snapshot updated to show remote version
}

#[tokio::test]
#[ignore]
async fn test_multiple_field_conflicts() {
    // TODO: implement test case for multiple conflicting fields
    // - Edit client locally (change name and email)
    // - Backend changes both name and email to different values
    // - refresh_snapshot() should return both fields in conflicts list
}

#[tokio::test]
#[ignore]
async fn test_on_backend_change_updates_cache() {
    // TODO: implement test case for on_backend_change callback
    // - Initialize LiveTable with cache populated
    // - Simulate backend change (update client record directly in SurrealDB)
    // - Call on_backend_change(id)
    // - Verify cache reflects updated backend value
}

#[tokio::test]
#[ignore]
async fn test_snapshot_time_tracking() {
    // TODO: implement test case for snapshot timestamp updates
    // - Create RecordEdit, note initial snapshot_time
    // - Wait briefly
    // - Backend changes, call refresh_snapshot()
    // - Verify snapshot_time is updated to newer value
    // - Useful for showing "editing stale data" warnings in UI
}

#[tokio::test]
#[ignore]
async fn test_conflict_resolution_keep_local() {
    // TODO: implement test case for user choosing to keep local changes despite conflict
    // - Edit client locally
    // - Backend changes same fields
    // - refresh_snapshot() detects conflict
    // - User decides to keep local - just call save()
    // - Verify local changes overwrite backend (last-write-wins)
}

#[tokio::test]
#[ignore]
async fn test_conflict_resolution_use_remote() {
    // TODO: implement test case for user choosing to discard local changes
    // - Edit client locally
    // - Backend changes same fields
    // - refresh_snapshot() detects conflict
    // - User decides to use remote - call revert()
    // - Verify local matches snapshot (remote value)
    // - No modifications remain
}

#[tokio::test]
#[ignore]
async fn test_conflict_with_callback() {
    // TODO: implement test case for on_remote_change callback integration
    // - Initialize LiveTable with on_remote_change callback
    // - Simulate backend change
    // - Verify callback invoked with correct record ID
    // - UI can then refresh or notify user
}

#[tokio::test]
#[ignore]
async fn test_edit_during_backend_change() {
    // TODO: implement test case for backend changing while user actively editing
    // - Start editing client
    // - Make some local changes
    // - Backend changes (simulated by another connection/process)
    // - Call on_backend_change to update cache
    // - RecordEdit still has local changes
    // - refresh_snapshot() shows conflicts
    // - User can decide how to proceed
}

#[tokio::test]
#[ignore]
async fn test_no_conflict_after_save() {
    // TODO: implement test case verifying save clears conflict state
    // - Edit client locally
    // - Backend changes same field (conflict)
    // - refresh_snapshot() shows conflict
    // - User edits to resolve, calls save()
    // - After save, snapshot and local should match
    // - No conflicts remaining
}
