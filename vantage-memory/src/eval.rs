//! Queries: the one pipeline both layers compile to.
//! Candidates → conditions → search → order → offset/limit.

pub mod compare;
mod condition;
mod order;
#[cfg(test)]
mod tests;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::SortDirection;

pub use condition::MemoryCondition;
pub use order::sort_rows;

#[derive(Clone, Default)]
pub struct Query {
    /// AND-ed together.
    pub conditions: Vec<MemoryCondition>,
    pub search: Option<String>,
    pub order: Vec<(String, SortDirection)>,
    pub offset: usize,
    pub limit: Option<usize>,
}

impl Query {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn filter(mut self, c: MemoryCondition) -> Self {
        self.conditions.push(c);
        self
    }
    pub fn search(mut self, text: impl Into<String>) -> Self {
        self.search = Some(text.into());
        self
    }
    pub fn order_by(mut self, path: impl Into<String>, dir: SortDirection) -> Self {
        self.order.push((path.into(), dir));
        self
    }
    pub fn window(mut self, offset: usize, limit: Option<usize>) -> Self {
        self.offset = offset;
        self.limit = limit;
        self
    }

    /// Resolve every `Deferred` condition.
    pub async fn resolve(mut self) -> Result<Self> {
        let mut out = Vec::with_capacity(self.conditions.len());
        for c in self.conditions {
            out.push(c.resolve().await?);
        }
        self.conditions = out;
        Ok(self)
    }
}

/// Whether a row passes every condition and the search.
pub fn matches_all(q: &Query, record: &Record<CborValue>) -> Result<bool> {
    for c in &q.conditions {
        if !c.matches(record)? {
            return Ok(false);
        }
    }
    Ok(match &q.search {
        Some(s) => MemoryCondition::Search(s.clone()).matches(record)?,
        None => true,
    })
}
