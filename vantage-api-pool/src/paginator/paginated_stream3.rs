use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use anyhow::Error;
use futures_core::{Future, Stream};
use serde_json::Value;
use tokio::task::JoinHandle;
use tracing::Instrument as _;

use crate::AwwPool;

enum StreamState {
    FetchingPage {
        handle: JoinHandle<Result<Value, Error>>,
        page: usize,
    },
    YieldingItems {
        items: Vec<Value>,
        index: usize,
        has_next: bool,
        next_page: usize,
    },
    Done,
}

pub struct PaginatedStream {
    pool: Arc<AwwPool>,
    endpoint: String,
    state: StreamState,
}

impl PaginatedStream {
    pub fn get(pool: Arc<AwwPool>, endpoint: String) -> Self {
        let url = format!("{}?page={}", endpoint, 1);
        let pool_clone = pool.clone();

        let handle = tokio::spawn(
            async move {
                let response = pool_clone.get(&url).await?;
                let body: Value = response.json().await?;
                Ok(body)
            }
            .in_current_span(),
        );

        Self {
            pool,
            endpoint,
            state: StreamState::FetchingPage { handle, page: 1 },
        }
    }

    pub fn prefetch(self, _pages: usize) -> Self {
        // No prefetching - ignored
        self
    }
}

impl Stream for PaginatedStream {
    type Item = Result<Value, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match &mut self.state {
                StreamState::FetchingPage { handle, page } => {
                    match Pin::new(handle).poll(cx) {
                        Poll::Ready(Ok(Ok(body))) => {
                            // Extract data items - avoid cloning the entire array
                            let items = match body.get("data").and_then(|v| v.as_array()) {
                                Some(arr) => arr.clone(),
                                None => Vec::new(),
                            };

                            if items.is_empty() {
                                self.state = StreamState::Done;
                                return Poll::Ready(None);
                            }

                            // Check if there's a next page
                            let has_next = if *page == 1 {
                                // On first page, check total_pages
                                if let Some(pagination) = body.get("pagination") {
                                    if let Some(total_pages) =
                                        pagination.get("total_pages").and_then(|v| v.as_u64())
                                    {
                                        *page < total_pages as usize
                                    } else {
                                        // Fallback to has_next
                                        pagination
                                            .get("has_next")
                                            .and_then(|v| v.as_bool())
                                            .unwrap_or(false)
                                    }
                                } else {
                                    false
                                }
                            } else {
                                // For subsequent pages, use has_next
                                body.get("pagination")
                                    .and_then(|p| p.get("has_next"))
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false)
                            };

                            let next_page = *page + 1;
                            self.state = StreamState::YieldingItems {
                                items,
                                index: 0,
                                has_next,
                                next_page,
                            };
                        }
                        Poll::Ready(Ok(Err(e))) => {
                            self.state = StreamState::Done;
                            return Poll::Ready(Some(Err(e)));
                        }
                        Poll::Ready(Err(e)) => {
                            self.state = StreamState::Done;
                            return Poll::Ready(Some(Err(e.into())));
                        }
                        Poll::Pending => return Poll::Pending,
                    }
                }

                StreamState::YieldingItems {
                    items,
                    index,
                    has_next,
                    next_page,
                } => {
                    if *index < items.len() {
                        // Return reference to avoid cloning - but we still need to clone for the API
                        let item = items[*index].clone();
                        *index += 1;
                        return Poll::Ready(Some(Ok(item)));
                    } else {
                        // Page exhausted - extract values before state change
                        let should_fetch_next = *has_next;
                        let page_num = *next_page;

                        if should_fetch_next {
                            // Extract values before borrowing issues
                            let endpoint = self.endpoint.clone();
                            let pool = self.pool.clone();

                            // Start fetching next page
                            let url = format!("{}?page={}", endpoint, page_num);

                            let handle = tokio::spawn(
                                async move {
                                    let response = pool.get(&url).await?;
                                    let body: Value = response.json().await?;
                                    Ok(body)
                                }
                                .in_current_span(),
                            );

                            self.state = StreamState::FetchingPage {
                                handle,
                                page: page_num,
                            };
                        } else {
                            self.state = StreamState::Done;
                            return Poll::Ready(None);
                        }
                    }
                }

                StreamState::Done => return Poll::Ready(None),
            }
        }
    }
}
