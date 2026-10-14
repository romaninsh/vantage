// Dummy crate so rustdoc-links can resolve items from dependencies.
pub use vantage_core;
pub use vantage_dataset;
pub use vantage_diorama;
pub use vantage_expressions;
pub use vantage_mongodb;
pub use vantage_rhai;
pub use vantage_sql;
pub use vantage_table;
pub use vantage_types;
pub use vantage_vista;

mod chapters;

#[cfg(test)]
mod tests {
    use crate::chapters::CHAPTERS;

    /// A chapter in SUMMARY.md that chapters.rs doesn't list is never compiled.
    #[test]
    fn every_summary_chapter_is_compiled() {
        let summary = include_str!("SUMMARY.md");
        for link in summary.split("](./").skip(1) {
            let path = link.split(')').next().unwrap_or_default();
            assert!(
                CHAPTERS.contains(&path),
                "{path} is in SUMMARY.md but not in chapters.rs"
            );
        }
    }
}
