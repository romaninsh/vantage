use std::sync::Arc;

use vantage_vista::{Condition, SortDirection};

use crate::dio::DioInner;

mod open;
mod seed;

/// Builder produced by [`Dio::table_scenery`](crate::Dio::table_scenery).
pub struct TableSceneryBuilder {
    pub(crate) dio: Arc<DioInner>,
    pub(crate) filters: Vec<Condition>,
    pub(crate) order: Option<(String, SortDirection)>,
    pub(crate) search: Option<String>,
    pub(crate) limit: Option<usize>,
    pub(crate) tail: Option<usize>,
    pub(crate) page_size: usize,
    pub(crate) titles_only: bool,
    pub(crate) demand: Option<Vec<String>>,
    pub(crate) sugar: Option<crate::scenery::sugar::Sugar>,
}

impl TableSceneryBuilder {
    pub(crate) fn new(dio: Arc<DioInner>) -> Self {
        Self {
            dio,
            filters: Vec::new(),
            order: None,
            search: None,
            limit: None,
            tail: None,
            page_size: 100,
            titles_only: false,
            demand: None,
            sugar: None,
        }
    }

    /// Read rows through `sugar`: its outputs are merged into every row
    /// this view reads — sort and filter included — and nothing reaches the
    /// cache. A sugared view reads rows of its own: it never shares them
    /// with another view, even one of the same query.
    pub fn sugar(mut self, sugar: crate::scenery::sugar::Sugar) -> Self {
        self.sugar = Some(sugar);
        self
    }

    /// Narrow the view by `condition`, for its whole life. Repeatable; the
    /// conditions are ANDed with each other and with the runtime
    /// [`set_filters`](super::TableScenery::set_filters). A paged view carries them
    /// into every chunk fetch and the master takes what it can; what it
    /// leaves over is evaluated over the whole copy when the Lens has an
    /// eager start, and fails the view loudly when it doesn't. An eager view
    /// evaluates them over its complete cache.
    pub fn filter(mut self, condition: Condition) -> Self {
        self.filters.push(condition);
        self
    }

    /// The view's order. Without one the view keeps the Dio's order.
    pub fn order(mut self, column: impl Into<String>, dir: SortDirection) -> Self {
        self.order = Some((column.into(), dir));
        self
    }

    /// The view's initial quicksearch text, as a later
    /// [`set_search`](super::TableScenery::set_search) would set it. Blank
    /// means none.
    pub fn search(mut self, text: impl Into<String>) -> Self {
        self.search = Some(text.into()).filter(|q| !q.trim().is_empty());
        self
    }

    /// Show at most the first `n` rows of the view's order. The view reads
    /// complete at `n`: [`has_more`](super::TableScenery::has_more) turns
    /// false and the total stops there. Exclusive with [`tail`](Self::tail).
    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    /// Show the last `n` rows of the view's order, still in that order: a
    /// log tail, which keeps showing the newest `n` as rows arrive at the
    /// end. Counted back from the end of the set once it is known, so nulls
    /// and ties sit exactly where the order puts them. A paged source that
    /// never states a total has no end to count from: the tail then reads
    /// the first `n` rows of the flipped order, and where nulls and ties
    /// fall is the master's call. Needs an order, the view's own or the
    /// Dio's, for as long as the view lives: [`open`](Self::open) refuses a
    /// tail without one, and clearing both with `set_order(None)` fails the
    /// view until an order is set again. Exclusive with [`limit`](Self::limit).
    pub fn tail(mut self, n: usize) -> Self {
        self.tail = Some(n);
        self
    }

    /// Hint range used by `request_load_more` and by the
    /// refresh-on-open initial fetch. Default 100.
    pub fn page_size(mut self, n: usize) -> Self {
        self.page_size = n;
        self
    }

    /// Project to the cheap title columns only — the dropdown / autocomplete
    /// shape. On an **augmented (two-pass)** table this suppresses the detail
    /// pass entirely: the scenery serves the list-pass rows (id + title columns)
    /// and never pays for per-row hydration, so a 10,000-row lookup opens a
    /// picker as cheaply as it lists. Same mechanic as a grid otherwise —
    /// `set_viewport` for the visible band. A
    /// `titles_only` picker and a full grid over the same query read
    /// distinct rows (the grid hydrates, the picker doesn't).
    pub fn titles_only(mut self) -> Self {
        self.titles_only = true;
        self
    }

    /// Declare which columns this view actually shows — its **demand**.
    /// Default (`None`) demands everything, the pre-demand behavior. Demand
    /// gates the AUGMENT detail pass only: the Dio unions the demands of its
    /// open views and fetches augment values only while some open view
    /// demands an augmented column (a tree of folder names never pays for
    /// folder sizes; the listing beside it does). Non-augment (list-pass)
    /// columns always flow regardless of demand. Recomputed naturally as
    /// views open and close; already-merged values stay when demand
    /// drains — they just stop refreshing.
    pub fn columns<I, S>(mut self, columns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.demand = Some(columns.into_iter().map(Into::into).collect());
        self
    }
}
