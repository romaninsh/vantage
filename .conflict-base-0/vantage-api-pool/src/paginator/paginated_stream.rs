use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use anyhow::Error;
use futures_core::{Future, Stream};
use serde_json::Value;
use tokio::task::JoinHandle;

use crate::AwwPool;

struct PageData {
    items: Vec<Value>,
    index: usize,
}

pub struct PaginatedStream {
    pool: Arc<AwwPool>,
    endpoint: String,
    prefetch: usize,
    next_page_to_fetch: usize,
    total_pages: Option<usize>,

    // FIFO queue of fetch tasks
    fetch_queue: VecDeque<JoinHandle<Result<(usize, Value), Error>>>,

    // Ready pages buffer
    ready_pages: VecDeque<PageData>,

    // Current page being consumed
    current_data: Option<PageData>,

    done: bool,
}

impl PaginatedStream {
    pub fn get(pool: Arc<AwwPool>, endpoint: String) -> Self {
        let mut stream = Self {
            pool,
            endpoint,
            prefetch: 1,
            next_page_to_fetch: 2,
            total_pages: None,
            fetch_queue: VecDeque::new(),
            ready_pages: VecDeque::new(),
            current_data: None,
            done: false,
        };

        // Start fetching first page
        stream.spawn_fetch(1);
        stream
    }

    pub fn prefetch(mut self, pages: usize) -> Self {
        self.prefetch = pages.max(1);
        self
    }

    fn spawn_fetch(&mut self, page: usize) {
        if self.done || (self.total_pages.is_some() && page > self.total_pages.unwrap()) {
            return;
        }

        let url = format!("{}?page={}", self.endpoint, page);
        let pool = self.pool.clone();

        let handle = tokio::spawn(async move {
            let response = pool.get(&url).await?;
            let body: Value = response.json().await?;
            Ok((page, body))
        });

        self.fetch_queue.push_back(handle);
    }

    fn ensure_prefetch(&mut self) {
        // Only prefetch if we know there are more pages or don't know total yet
        if let Some(total) = self.total_pages {
            if self.next_page_to_fetch > total {
                return; // No more pages to fetch
            }
        }

        let active_fetches = self.fetch_queue.len();
        let ready_pages = self.ready_pages.len();

        // prefetch should mean pages ready ahead of current consumption
        // We want: ready_pages + active_fetches >= prefetch
        let total_ahead = active_fetches + ready_pages;
        let needed = self.prefetch.saturating_sub(total_ahead);

        for _ in 0..needed {
            // Double-check we haven't exceeded total pages
            if let Some(total) = self.total_pages {
                if self.next_page_to_fetch > total {
                    break;
                }
            }

            self.spawn_fetch(self.next_page_to_fetch);
            self.next_page_to_fetch += 1;
        }
    }

    fn poll_fetch_queue(&mut self, cx: &mut Context<'_>) -> Result<bool, Error> {
        let mut made_progress = false;

        while let Some(mut handle) = self.fetch_queue.pop_front() {
            match Pin::new(&mut handle).poll(cx) {
                Poll::Ready(Ok(Ok((page, body)))) => {
                    made_progress = true;

                    // Extract pagination info from first page
                    if page == 1 {
                        if let Some(pagination) = body.get("pagination") {
                            self.total_pages = pagination
                                .get("total_pages")
                                .and_then(|v| v.as_u64())
                                .map(|v| v as usize);
                        }
                    }

                    // Extract data items
                    if let Some(data) = body.get("data").and_then(|v| v.as_array()) {
                        if !data.is_empty() {
                            self.ready_pages.push_back(PageData {
                                items: data.clone(),
                                index: 0,
                            });
                        }
                    }
                }
                Poll::Ready(Ok(Err(e))) => return Err(e),
                Poll::Ready(Err(e)) => return Err(e.into()),
                Poll::Pending => {
                    // Put it back at the front
                    self.fetch_queue.push_front(handle);
                    break;
                }
            }
        }

        Ok(made_progress)
    }
}

impl Stream for PaginatedStream {
    type Item = Result<Value, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.done {
            return Poll::Ready(None);
        }

        loop {
            // Try to get next item from current page
            if let Some(ref mut page_data) = self.current_data {
                if page_data.index < page_data.items.len() {
                    let item = page_data.items[page_data.index].clone();
                    page_data.index += 1;

                    // Ensure we have enough pages prefetched
                    self.ensure_prefetch();

                    return Poll::Ready(Some(Ok(item)));
                } else {
                    // Current page exhausted, move to next
                    self.current_data = None;
                }
            }

            // Try to get next ready page
            if let Some(page_data) = self.ready_pages.pop_front() {
                self.current_data = Some(page_data);
                continue;
            }

            // Poll fetch queue for new pages
            match self.poll_fetch_queue(cx) {
                Ok(made_progress) => {
                    if made_progress {
                        continue; // Try again with new data
                    }
                }
                Err(e) => return Poll::Ready(Some(Err(e))),
            }

            // Check if we're truly done
            if self.fetch_queue.is_empty() {
                if let Some(total) = self.total_pages {
                    if self.next_page_to_fetch > total {
                        self.done = true;
                        return Poll::Ready(None);
                    }
                    // If we know total pages but haven't fetched them all, fetch more
                    self.ensure_prefetch();
                } else {
                    // No active fetches and no ready data - we're done
                    self.done = true;
                    return Poll::Ready(None);
                }
            }

            // Still have pending fetches
            return Poll::Pending;
        }
    }
}
