mod batch;
pub use batch::*;
use gpui_component::IconName;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Page {
    GolfClub,
    Batch,
    BatchForm,
    Tag,
    BatchDetail(String), // batch_id
}

impl Page {
    pub fn name(&self) -> String {
        match self {
            Page::GolfClub => "Golf Club".to_string(),
            Page::Batch => "Batch".to_string(),
            Page::BatchForm => "Add Batch".to_string(),
            Page::Tag => "Tag".to_string(),
            Page::BatchDetail(id) => format!("Details: {}", id.split(':').last().unwrap_or(id)),
        }
    }

    pub fn icon(&self) -> IconName {
        match self {
            Page::GolfClub => IconName::Map,
            Page::Batch => IconName::Folder, // Using Folder instead of Package
            Page::BatchForm => IconName::Plus,
            Page::Tag => IconName::Star, // Using Star instead of Tag
            Page::BatchDetail(_) => IconName::Info,
        }
    }
}
