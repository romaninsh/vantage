//! Pipeline validation and the observation-args accessor for the `mongo:`
//! block.

use bson::Document;
use vantage_core::{Result, VantageError, error};
use vantage_vista::VistaSpec;

use super::{MongoBlock, MongoTableExtras};

impl MongoBlock {
    /// The `pipeline` stages as BSON documents; `Ok(None)` without one. Each
    /// stage must be a single `$`-prefixed operator, and the pipeline must be
    /// read-only: no `$out` or `$merge`.
    pub fn pipeline_stages(&self) -> Result<Option<Vec<Document>>> {
        let Some(stages) = &self.pipeline else {
            return Ok(None);
        };
        let stages = stages
            .iter()
            .enumerate()
            .map(|(index, stage)| {
                if !stage.is_object() {
                    return Err(not_a_stage(index));
                }
                bson::to_document(stage).map_err(|e| {
                    error!(
                        "pipeline stage is not a BSON document",
                        stage = index,
                        details = e.to_string()
                    )
                })
            })
            .collect::<Result<Vec<_>>>()?;
        check_stages(&stages)?;
        Ok(Some(stages))
    }
}

/// Stages that write their input to a collection; a pipeline view only reads.
const WRITING_STAGES: [&str; 2] = ["$out", "$merge"];

/// A pipeline must not be empty, each stage must be a document with a
/// single `$`-prefixed operator key, and no stage may write (`$out`,
/// `$merge`).
pub(crate) fn check_stages(stages: &[Document]) -> Result<()> {
    if stages.is_empty() {
        return Err(error!("pipeline must have at least one stage"));
    }
    for (index, stage) in stages.iter().enumerate() {
        if stage.len() != 1 || !stage.keys().all(|k| k.starts_with('$')) {
            return Err(not_a_stage(index));
        }
        if let Some(op) = stage.keys().find(|k| WRITING_STAGES.contains(&k.as_str())) {
            return Err(error!(
                "a pipeline view is read-only and cannot write with `$out` or `$merge`",
                stage = index,
                operator = op.as_str()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use bson::doc;

    use super::*;

    #[test]
    fn writing_stages_are_refused() {
        for writer in [
            doc! { "$out": "copy" },
            doc! { "$merge": { "into": "copy" } },
        ] {
            let stages = vec![doc! { "$match": {} }, writer.clone()];
            let msg = check_stages(&stages).unwrap_err().to_string();
            assert!(msg.contains("read-only"), "{writer}: {msg}");
            assert!(msg.contains("stage: 1"), "{writer}: {msg}");
        }
        assert!(check_stages(&[doc! { "$match": {} }]).is_ok());
    }
}

pub(crate) fn not_a_stage(index: usize) -> VantageError {
    error!(
        "pipeline stage must be a map with one `$` operator key",
        stage = index
    )
}

/// Observation args carried on the driver block ("" = unset by convention).
///
/// A host that opens vistas for an observation fills `MongoBlock::args`
/// before building; this reads them back off a spec without the host naming
/// the driver's block type. SurrealDB's spec has a trait of the same shape.
pub trait DriverBlockArgs {
    /// The `(name, value)` pairs on the spec's `mongo:` block, in order;
    /// empty when the spec has no block or no args.
    fn driver_block_args(&self) -> Vec<(String, String)>;
}

impl<C, R> DriverBlockArgs for VistaSpec<MongoTableExtras, C, R> {
    fn driver_block_args(&self) -> Vec<(String, String)> {
        self.driver
            .mongo
            .as_ref()
            .map(|b| b.args.clone())
            .unwrap_or_default()
    }
}
