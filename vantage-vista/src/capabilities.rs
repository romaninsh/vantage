use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VistaCapabilities {
    pub can_count: bool,
    pub can_insert: bool,
    pub can_update: bool,
    pub can_delete: bool,
    pub can_subscribe: bool,
    pub can_invalidate: bool,
    pub paginate_kind: PaginateKind,
}

impl Default for VistaCapabilities {
    fn default() -> Self {
        Self {
            can_count: false,
            can_insert: false,
            can_update: false,
            can_delete: false,
            can_subscribe: false,
            can_invalidate: false,
            paginate_kind: PaginateKind::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaginateKind {
    None,
    Offset,
    Cursor,
}
