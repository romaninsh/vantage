/// Handle for temporary conditions that can be removed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConditionHandle(pub(crate) i64);

impl ConditionHandle {
    pub(crate) fn new(id: i64) -> Self {
        Self(id)
    }

    pub(crate) fn id(&self) -> i64 {
        self.0
    }
}
