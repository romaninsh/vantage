use chrono;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants},
    form::{form_field, v_form},
    h_flex,
    input::{InputState, TextInput},
    sidebar::{Sidebar, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem},
    tab::{Tab, TabBar},
    table::Table,
    v_flex, ActiveTheme as _, Icon, IconName, Sizable, Size,
};
use std::fs;

use crate::page::*;

pub struct AdminApp {
    active_page: Page,
    open_tabs: Vec<Page>,
    active_tab: usize,
    batch_data: Option<Vec<BatchItem>>,
    batch_table: Option<Entity<Table<BatchTableDelegate>>>,
    batch_details: std::collections::HashMap<String, Vec<(String, String)>>,
    pub(crate) pending_detail_request: Option<String>,
    batch_form_name: Option<Entity<InputState>>,
    batch_form_golf_course: Option<Entity<InputState>>,
}

impl AdminApp {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let batch_data = Self::load_batch_data();

        let batch_table = if let Some(ref batches) = batch_data {
            let table =
                cx.new(|cx| Table::new(BatchTableDelegate::new(batches.clone()), window, cx));
            // Configure table for proper scrolling and display
            table.update(cx, |table, cx| {
                table.set_stripe(true, cx);
                table.set_size(Size::XSmall, cx);
                table.col_movable = false; // Disable column reordering
                cx.notify();
            });
            Some(table)
        } else {
            None
        };

        Self {
            active_page: Page::GolfClub,
            open_tabs: vec![Page::GolfClub],
            active_tab: 0,
            batch_data,
            batch_table,
            batch_details: std::collections::HashMap::new(),
            pending_detail_request: None,
            batch_form_name: None,
            batch_form_golf_course: None,
        }
    }

    fn save_batch_item(
        &mut self,
        name: String,
        golf_course: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let new_batch = BatchItem::new(
            name,
            golf_course,
            0,
            chrono::Utc::now().format("%Y-%m-%d").to_string(),
        );

        // Add to batch data
        if let Some(ref mut batches) = self.batch_data {
            batches.push(new_batch);
        } else {
            self.batch_data = Some(vec![new_batch]);
        }

        // Update the table
        if let Some(ref batches) = self.batch_data {
            let new_table =
                cx.new(|cx| Table::new(BatchTableDelegate::new(batches.clone()), window, cx));
            new_table.update(cx, |table, cx| {
                table.set_stripe(true, cx);
                table.set_size(Size::XSmall, cx);
                table.col_movable = false;
                cx.notify();
            });
            self.batch_table = Some(new_table);
        }
    }

    fn load_batch_data() -> Option<Vec<BatchItem>> {
        let yaml_content = fs::read_to_string("data/batch.yaml").ok()?;
        let batch_data: BatchData = serde_yaml::from_str(&yaml_content).ok()?;
        Some(batch_data.batch)
    }

    pub fn view(window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self::new(window, cx))
    }

    fn open_tab(&mut self, page: Page, _: &mut Window, cx: &mut Context<Self>) {
        if !self.open_tabs.contains(&page) {
            self.open_tabs.push(page.clone());
        }
        self.active_tab = self.open_tabs.iter().position(|p| p == &page).unwrap();
        self.active_page = page;
        cx.notify();
    }

    fn close_tab(&mut self, tab_index: usize, _: &mut Window, cx: &mut Context<Self>) {
        if tab_index < self.open_tabs.len() {
            // Don't allow closing the last tab or main navigation tabs
            let page = &self.open_tabs[tab_index];

            // Only allow closing detail tabs and form tabs
            if (matches!(page, Page::BatchDetail(_)) || matches!(page, Page::BatchForm))
                && self.open_tabs.len() > 1
            {
                // Clean up any associated data if it's a BatchDetail tab
                if let Page::BatchDetail(batch_id) = page {
                    self.batch_details.remove(batch_id);
                }

                self.open_tabs.remove(tab_index);

                // Adjust active tab if necessary
                if self.active_tab >= self.open_tabs.len() {
                    self.active_tab = self.open_tabs.len() - 1;
                } else if self.active_tab > tab_index {
                    self.active_tab -= 1;
                }

                self.active_page = self.open_tabs[self.active_tab].clone();
                cx.notify();
            }
        }
    }

    fn set_active_tab(&mut self, tab_index: usize, _: &mut Window, cx: &mut Context<Self>) {
        if tab_index < self.open_tabs.len() {
            self.active_tab = tab_index;
            self.active_page = self.open_tabs[tab_index].clone();
            cx.notify();
        }
    }

    fn open_batch_details(
        &mut self,
        batch_id: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Find the batch data
        if let Some(ref batches) = self.batch_data {
            if let Some(batch) = batches.iter().find(|b| b.id() == batch_id) {
                let page = Page::BatchDetail(batch_id.clone());

                // Create the detail data if it doesn't exist
                if !self.batch_details.contains_key(&batch_id) {
                    let details = create_batch_details(batch);
                    self.batch_details.insert(batch_id.clone(), details);
                }

                // Open the tab
                if !self.open_tabs.contains(&page) {
                    self.open_tabs.push(page.clone());
                }
                self.active_tab = self.open_tabs.iter().position(|p| p == &page).unwrap();
                self.active_page = page;
                cx.notify();
            }
        }
    }

    fn render_page_content(
        &mut self,
        page: Page,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        match page {
            Page::GolfClub => v_flex()
                .gap_4()
                .p_6()
                .child(
                    div()
                        .text_2xl()
                        .font_weight(FontWeight::BOLD)
                        .child("Golf Club Management"),
                )
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child("Golf club management features will be implemented here."),
                ),
            Page::Batch => v_flex()
                .size_full()
                .gap_4()
                .child(
                    div()
                        .p_6()
                        .text_2xl()
                        .font_weight(FontWeight::BOLD)
                        .child("Batch Management"),
                )
                .child(
                    div().px_6().child(
                        Button::new("add-batch-btn")
                            .primary()
                            .child("Add New Batch")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_tab(Page::BatchForm, window, cx);
                            })),
                    ),
                )
                .child(if let Some(ref table) = self.batch_table {
                    div()
                        .flex_1()
                        .px_6()
                        .pb_6()
                        .w_full()
                        .min_h(px(500.))
                        .child(table.clone())
                } else {
                    v_flex()
                        .gap_2()
                        .p_4()
                        .child("No batch table available")
                        .text_color(cx.theme().muted_foreground)
                }),
            Page::BatchForm => {
                // Initialize form inputs lazily
                if self.batch_form_name.is_none() {
                    self.batch_form_name =
                        Some(cx.new(|cx| {
                            InputState::new(_window, cx).placeholder("Enter batch name...")
                        }));
                }
                if self.batch_form_golf_course.is_none() {
                    self.batch_form_golf_course = Some(cx.new(|cx| {
                        InputState::new(_window, cx).placeholder("Enter golf course name...")
                    }));
                }

                v_flex()
                    .size_full()
                    .gap_4()
                    .child(
                        div()
                            .p_6()
                            .text_2xl()
                            .font_weight(FontWeight::BOLD)
                            .child("Add New Batch"),
                    )
                    .child(
                        div().p_6().child(
                            v_form()
                                .child(
                                    form_field()
                                        .label("Batch Name")
                                        .child(TextInput::new(
                                            self.batch_form_name.as_ref().unwrap(),
                                        ))
                                        .required(true),
                                )
                                .child(
                                    form_field()
                                        .label("Golf Course")
                                        .child(TextInput::new(
                                            self.batch_form_golf_course.as_ref().unwrap(),
                                        ))
                                        .required(true),
                                )
                                .child(
                                    form_field().child(
                                        h_flex()
                                            .gap_3()
                                            .mt_4()
                                            .child(
                                                Button::new("save-batch")
                                                    .primary()
                                                    .child("Save Batch")
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            println!("Save batch button clicked");

                                                            // Get form values
                                                            let name = if let Some(ref input) = this.batch_form_name {
                                                                input.read(cx).value().to_string()
                                                            } else {
                                                                String::new()
                                                            };

                                                            let golf_course = if let Some(ref input) = this.batch_form_golf_course {
                                                                input.read(cx).value().to_string()
                                                            } else {
                                                                String::new()
                                                            };

                                                            // Validate form
                                                            if name.trim().is_empty() || golf_course.trim().is_empty() {
                                                                println!("Form validation failed: empty fields");
                                                                return;
                                                            }

                                                            // Save the batch data
                                                            this.save_batch_item(name, golf_course, window, cx);

                                                            // Close tab first
                                                            println!("Open tabs: {:?}", this.open_tabs);
                                                            if let Some(index) = this
                                                                .open_tabs
                                                                .iter()
                                                                .position(|p| {
                                                                    matches!(p, Page::BatchForm)
                                                                })
                                                            {
                                                                println!(
                                                                    "Closing tab at index: {}",
                                                                    index
                                                                );
                                                                this.close_tab(index, window, cx);
                                                            } else {
                                                                println!("BatchForm tab not found in open tabs");
                                                            }

                                                            // Clear form state after closing tab
                                                            this.batch_form_name = None;
                                                            this.batch_form_golf_course = None;
                                                        },
                                                    )),
                                            )
                                            .child(
                                                Button::new("cancel-batch")
                                                    .outline()
                                                    .child("Cancel")
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            println!("Cancel batch button clicked");

                                                            // Close tab first
                                                            println!("Open tabs: {:?}", this.open_tabs);
                                                            if let Some(index) = this
                                                                .open_tabs
                                                                .iter()
                                                                .position(|p| {
                                                                    matches!(p, Page::BatchForm)
                                                                })
                                                            {
                                                                println!(
                                                                    "Closing tab at index: {}",
                                                                    index
                                                                );
                                                                this.close_tab(index, window, cx);
                                                            } else {
                                                                println!("BatchForm tab not found in open tabs");
                                                            }

                                                            // Clear form state after closing tab
                                                            this.batch_form_name = None;
                                                            this.batch_form_golf_course = None;
                                                        },
                                                    )),
                                            ),
                                    ),
                                ),
                        ),
                    )
            }
            Page::BatchDetail(batch_id) => {
                v_flex()
                    .size_full()
                    .gap_4()
                    .child(
                        div()
                            .p_6()
                            .text_2xl()
                            .font_weight(FontWeight::BOLD)
                            .child(format!(
                                "Batch Details: {}",
                                batch_id.split(':').last().unwrap_or(&batch_id)
                            )),
                    )
                    .child(if let Some(details) = self.batch_details.get(&batch_id) {
                        v_flex().flex_1().px_6().pb_6().w_full().gap_3().children(
                            details.iter().map(|(key, value)| {
                                h_flex()
                                    .w_full()
                                    .justify_between()
                                    .items_center()
                                    .px_4()
                                    .py_3()
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(cx.theme().muted_foreground)
                                            .child(key.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().foreground)
                                            .child(value.clone()),
                                    )
                            }),
                        )
                    } else {
                        div()
                            .p_6()
                            .text_color(cx.theme().muted_foreground)
                            .child("Loading batch details...")
                    })
            }
            Page::Tag => v_flex()
                .gap_4()
                .p_6()
                .child(
                    div()
                        .text_2xl()
                        .font_weight(FontWeight::BOLD)
                        .child("Tag Management"),
                )
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child("Tag management features will be implemented here."),
                ),
        }
    }
}

impl Render for AdminApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Check if we have a pending detail request
        if let Some(batch_id) = self.pending_detail_request.take() {
            self.open_batch_details(batch_id, window, cx);
        }
        h_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                // Sidebar
                Sidebar::left()
                    .width(px(250.))
                    .header(
                        SidebarHeader::new().child(
                            h_flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded_lg()
                                        .bg(cx.theme().primary)
                                        .text_color(cx.theme().primary_foreground)
                                        .size_8()
                                        .child(Icon::new(IconName::Settings)),
                                )
                                .child(
                                    v_flex()
                                        .gap_0()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child("Admin System"),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child("Golf Management"),
                                        ),
                                ),
                        ),
                    )
                    .child(
                        SidebarGroup::new("Management").child(
                            SidebarMenu::new()
                                .child(
                                    SidebarMenuItem::new("Golf Club")
                                        .icon(Page::GolfClub.icon())
                                        .active(self.active_page == Page::GolfClub)
                                        .on_click(cx.listener(
                                            move |this, _: &ClickEvent, window, cx| {
                                                this.open_tab(Page::GolfClub, window, cx);
                                            },
                                        )),
                                )
                                .child(
                                    SidebarMenuItem::new("Batch")
                                        .icon(Page::Batch.icon())
                                        .active(self.active_page == Page::Batch)
                                        .on_click(cx.listener(
                                            move |this, _: &ClickEvent, window, cx| {
                                                this.open_tab(Page::Batch, window, cx);
                                            },
                                        )),
                                )
                                .child(
                                    SidebarMenuItem::new("Tag")
                                        .icon(Page::Tag.icon())
                                        .active(self.active_page == Page::Tag)
                                        .on_click(cx.listener(
                                            move |this, _: &ClickEvent, window, cx| {
                                                this.open_tab(Page::Tag, window, cx);
                                            },
                                        )),
                                ),
                        ),
                    ),
            )
            .child(
                // Main content area
                v_flex()
                    .flex_1()
                    .w_full()
                    .h_full()
                    .overflow_hidden()
                    .child(
                        // Tab bar
                        div()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().background)
                            .child(
                                TabBar::new("main-tabs")
                                    .selected_index(self.active_tab)
                                    .on_click(cx.listener(
                                        |this, &tab_index: &usize, window, cx| {
                                            this.set_active_tab(tab_index, window, cx);
                                        },
                                    ))
                                    .children(self.open_tabs.iter().enumerate().map(
                                        |(i, page)| {
                                            let mut tab = Tab::new(page.name());

                                            // Add close button for detail tabs (not main pages)
                                            if matches!(page, Page::BatchDetail(_)) {
                                                tab = tab.suffix(
                                                    div()
                                                        .ml_2()
                                                        .child(
                                                            Button::new(("close-tab", i))
                                                                .ghost()
                                                                .xsmall()
                                                                .icon(IconName::Close)
                                                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                                                    this.close_tab(i, window, cx);
                                                                }))
                                                        )
                                                        .into_any_element()
                                                );
                                            }

                                            tab
                                        },
                                    )),
                            ),
                    )
                    .child(
                        // Tab content
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .child(self.render_page_content(self.active_page.clone(), window, cx)),
                    ),
            )
    }
}
