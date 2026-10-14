//! The leading-window probe of `check_advertised`: a shell that lists
//! answers `fetch_window(0, n)` with `min(n, listed)` rows of the set, the
//! first in the set's order when an order is applied, whether or not it
//! advertises `Window`.

use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_vista::{
    Capability, Column, ColumnFlag, Page, SortDirection, TableShell, Vista, VistaCapabilities,
    VistaMetadata, contract, mocks::MockShell, unsupported,
};

use super::{product_shell, rec, t};

/// Lists, but refuses every window.
#[derive(Clone)]
struct NoWindow {
    inner: Box<dyn TableShell>,
}

vantage_vista::forward_table_shell!(NoWindow, inner, {
    async fn fetch_window(&self, _offset: usize, _limit: usize) -> Result<Page> {
        unsupported(Capability::Window)
    }
});

#[tokio::test]
#[should_panic(expected = "every shell that lists answers the leading window")]
async fn a_refused_leading_window_fails_check_advertised() {
    let shell = product_shell().with_capabilities(VistaCapabilities::new());
    let vista = Vista::new("product", shell).map_shell(|inner| NoWindow { inner });
    contract::check_advertised(&vista).await;
}

/// The products without `Window`, answering only the leading window from
/// their listing.
#[derive(Clone)]
struct Leading {
    inner: Box<dyn TableShell>,
}

vantage_vista::forward_table_shell!(Leading, inner, {
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        Ok(Page::from_listing(self.inner.list().await?, offset, limit))
    }
});

#[tokio::test]
async fn a_leading_window_without_window_passes() {
    let shell = product_shell();
    let caps = shell.capabilities().without(Capability::Window);
    let vista =
        Vista::new("product", shell.with_capabilities(caps)).map_shell(|inner| Leading { inner });
    assert!(!vista.capabilities().has(Capability::Window));
    contract::check_advertised(&vista).await;
}

/// Answers every window with the whole listing.
#[derive(Clone)]
struct Unlimited {
    inner: Box<dyn TableShell>,
}

vantage_vista::forward_table_shell!(Unlimited, inner, {
    async fn fetch_window(&self, _offset: usize, _limit: usize) -> Result<Page> {
        Ok(Page::new(self.inner.list().await?.into_iter().collect()))
    }
});

#[tokio::test]
#[should_panic(expected = "the limit is not applied")]
async fn an_ignored_limit_fails() {
    let vista = Vista::new("product", product_shell()).map_shell(|inner| Unlimited { inner });
    contract::check_advertised(&vista).await;
}

/// Answers the leading window from the end of the listing.
#[derive(Clone)]
struct Reversed {
    inner: Box<dyn TableShell>,
}

vantage_vista::forward_table_shell!(Reversed, inner, {
    async fn fetch_window(&self, _offset: usize, limit: usize) -> Result<Page> {
        let rows = self.inner.list().await?.into_iter().rev().take(limit);
        Ok(Page::new(rows.collect()))
    }
});

#[tokio::test]
#[should_panic(expected = "not the first values in the set's order")]
async fn a_window_out_of_the_set_order_fails() {
    let vista = Vista::new("product", product_shell())
        .map_shell(|inner| Reversed { inner })
        .with_order("id", SortDirection::Ascending)
        .unwrap();
    contract::check_advertised(&vista).await;
}

/// Answers the leading window in the listed order, except that the first
/// two rows swap places.
#[derive(Clone)]
struct SwapFirstTwo {
    inner: Box<dyn TableShell>,
}

vantage_vista::forward_table_shell!(SwapFirstTwo, inner, {
    async fn fetch_window(&self, _offset: usize, limit: usize) -> Result<Page> {
        let mut rows: Vec<_> = self.inner.list().await?.into_iter().collect();
        if rows.len() >= 2 {
            rows.swap(0, 1);
        }
        rows.truncate(limit);
        Ok(Page::new(rows))
    }
});

/// Two items share `price` 5 and lead the set ordered by price, so the
/// window may return them in either order.
#[tokio::test]
async fn tied_rows_may_come_back_in_either_order() {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
        .with_column(Column::new("price", "i64").with_flag(ColumnFlag::Orderable))
        .with_id_column("id");
    let item = |id: &str, price: i64| rec(&[("id", t(id)), ("price", price.into())]);
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_record("a", item("a", 5))
        .with_record("b", item("b", 5))
        .with_record("c", item("c", 9));
    let vista = Vista::new("item", shell)
        .map_shell(|inner| SwapFirstTwo { inner })
        .with_order("price", SortDirection::Ascending)
        .unwrap();

    let listed: Vec<String> = vista.list_values().await.unwrap().into_keys().collect();
    let window = vista.fetch_window(0, 2).await.unwrap();
    let windowed: Vec<&str> = window.rows.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(listed, ["a", "b", "c"]);
    assert_eq!(windowed, ["b", "a"], "the window swaps the tied rows");
    contract::check_advertised(&vista).await;
}
