use gpui::*;
use gpui_component::{
    popup_menu::PopupMenu,
    table::{Column, ColumnSort, Table, TableDelegate},
    ActiveTheme,
};
use serde::Deserialize;
use std::ops::Range;

// Action for showing batch details
#[derive(Action, Clone, PartialEq, Eq, Deserialize, Debug)]
#[action(namespace = rust_admin, no_json)]
pub struct ShowBatchDetails(pub String);

#[derive(Debug, Clone, Deserialize)]
pub struct BatchData {
    pub batch: Vec<BatchItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BatchItem {
    id: String,
    name: String,
    golf_course: String,
    total_tags: u32,
    created: String,
}

impl BatchItem {
    pub fn new(name: String, golf_course: String, total_tags: u32, created: String) -> Self {
        Self {
            id: format!("batch_{}", chrono::Utc::now().timestamp()),
            name,
            golf_course,
            total_tags,
            created,
        }
    }
    pub fn id(&self) -> &str {
        &self.id
    }
}

pub struct BatchTableDelegate {
    batches: Vec<BatchItem>,
    columns: Vec<Column>,
    visible_rows: Range<usize>,
    visible_cols: Range<usize>,
}

impl BatchTableDelegate {
    pub fn new(batches: Vec<BatchItem>) -> Self {
        Self {
            batches,
            columns: vec![
                Column::new("id", "ID")
                    .width(200.)
                    .fixed(gpui_component::table::ColumnFixed::Left),
                Column::new("name", "Name").width(200.).sortable(),
                Column::new("golf_course", "Golf Course")
                    .width(250.)
                    .sortable(),
                Column::new("total_tags", "Total Tags")
                    .width(120.)
                    .sortable()
                    .text_right(),
                Column::new("created", "Created").width(200.).sortable(),
            ],
            visible_rows: Range::default(),
            visible_cols: Range::default(),
        }
    }
}

impl TableDelegate for BatchTableDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.batches.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> &Column {
        &self.columns[col_ix]
    }

    fn render_th(
        &self,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<Table<Self>>,
    ) -> impl IntoElement {
        info!("Rendering header for column {}", col_ix);
        let col = &self.columns[col_ix];
        div()
            .px_2()
            .py_1()
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .bg(cx.theme().secondary)
            .text_color(cx.theme().secondary_foreground)
            .child(col.name.clone())
    }

    fn render_td(
        &self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<Table<Self>>,
    ) -> impl IntoElement {
        let batch = &self.batches[row_ix];
        let column = &self.columns[col_ix];

        let content = match column.key.as_ref() {
            "id" => batch.id.clone(),
            "name" => batch.name.clone(),
            "golf_course" => batch.golf_course.clone(),
            "total_tags" => batch.total_tags.to_string(),
            "created" => batch.created.clone(),
            _ => String::new(),
        };

        div()
            .px_2()
            .py_0p5()
            .text_xs()
            .text_color(cx.theme().foreground)
            .child(content)
    }

    fn visible_rows_changed(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        _: &mut Context<Table<Self>>,
    ) {
        self.visible_rows = range;
    }

    fn visible_columns_changed(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        _: &mut Context<Table<Self>>,
    ) {
        self.visible_cols = range;
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        _: &mut Context<Table<Self>>,
    ) {
        if let Some(col) = self.columns.get(col_ix) {
            match col.key.as_ref() {
                "id" => self.batches.sort_by(|a, b| match sort {
                    ColumnSort::Descending => b.id.cmp(&a.id),
                    _ => a.id.cmp(&b.id),
                }),
                "name" => self.batches.sort_by(|a, b| match sort {
                    ColumnSort::Descending => b.name.cmp(&a.name),
                    _ => a.name.cmp(&b.name),
                }),
                "golf_course" => self.batches.sort_by(|a, b| match sort {
                    ColumnSort::Descending => b.golf_course.cmp(&a.golf_course),
                    _ => a.golf_course.cmp(&b.golf_course),
                }),
                "total_tags" => self.batches.sort_by(|a, b| match sort {
                    ColumnSort::Descending => b.total_tags.cmp(&a.total_tags),
                    _ => a.total_tags.cmp(&b.total_tags),
                }),
                "created" => self.batches.sort_by(|a, b| match sort {
                    ColumnSort::Descending => b.created.cmp(&a.created),
                    _ => a.created.cmp(&b.created),
                }),
                _ => {}
            }
        }
    }

    fn context_menu(
        &self,
        row_ix: usize,
        menu: PopupMenu,
        _window: &Window,
        _cx: &App,
    ) -> PopupMenu {
        let batch = &self.batches[row_ix];
        menu.menu("Details", Box::new(ShowBatchDetails(batch.id.clone())))
    }
}

// Helper function to create batch details
pub fn create_batch_details(batch: &BatchItem) -> Vec<(String, String)> {
    vec![
        ("ID".to_string(), batch.id.clone()),
        ("Name".to_string(), batch.name.clone()),
        (
            "Golf Course".to_string(),
            batch.golf_course.replace("golf_course:", ""),
        ),
        ("Total Tags".to_string(), batch.total_tags.to_string()),
        ("Created".to_string(), batch.created.clone()),
    ]
}
