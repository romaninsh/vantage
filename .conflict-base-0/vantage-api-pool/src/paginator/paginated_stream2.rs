use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use anyhow::Error;
use futures_core::{Future, Stream};
use serde_json::Value;
use tokio::task::JoinHandle;

use crate::AwwPool;

enum StreamState {
    Initial,
    FetchingPage(JoinHandle<Result<Value, Error>>),
    YieldingItems {
        items: Vec<Value>,
        index: usize,
        next_page: Option<usize>,
    },
    FetchingNext {
        items: Vec<Value>,
        index: usize,
        next_fetch: JoinHandle<Result<Value, Error>>,
    },
    Done,
}

pub struct PaginatedStream {
    pool: Arc<AwwPool>,
    endpoint: String,
    current_page: usize,
    total_pages: Option<usize>,
    state: StreamState,
}

impl PaginatedStream {
    pub fn get(pool: Arc<AwwPool>, endpoint: String) -> Self {
        let mut stream = Self {
            pool,
            endpoint,
            current_page: 1,
            total_pages: None,
            state: StreamState::Initial,
        };

        // Start fetching first page
        let handle = stream.spawn_fetch_page(1);
        stream.state = StreamState::FetchingPage(handle);
        stream
    }

    pub fn prefetch(self, _pages: usize) -> Self {
        // Ignored in this implementation - always fetch one page ahead
        self
    }

    fn spawn_fetch_page(&self, page: usize) -> JoinHandle<Result<Value, Error>> {
        let url = format!("{}?page={}", self.endpoint, page);
        let pool = self.pool.clone();

        tokio::spawn(async move {
            let response = pool.get(&url).await?;
            let body: Value = response.json().await?;
            Ok(body)
        })
    }

    fn extract_page_data(&mut self, body: &Value) -> (Vec<Value>, Option<usize>) {
        // Extract pagination info if this is the first page
        if self.current_page == 1 {
            if let Some(pagination) = body.get("pagination") {
                self.total_pages = pagination
                    .get("total_pages")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);
            }
        }

        // Extract data items
        let items = body
            .get("data")
            .and_then(|v| v.as_array())
            .map(|arr| arr.clone())
            .unwrap_or_default();

        // Determine next page
        let next_page = if let Some(total) = self.total_pages {
            if self.current_page < total {
                Some(self.current_page + 1)
            } else {
                None
            }
        } else {
            // Check has_next if available
            body.get("pagination")
                .and_then(|p| p.get("has_next"))
                .and_then(|v| v.as_bool())
                .map(|has_next| {
                    if has_next {
                        Some(self.current_page + 1)
                    } else {
                        None
                    }
                })
                .unwrap_or(Some(self.current_page + 1)) // Assume there might be more
        };

        (items, next_page)
    }
}

impl Stream for PaginatedStream {
    type Item = Result<Value, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match &mut self.state {
                StreamState::Initial => {
                    // This should never happen as we initialize in FetchingPage state
                    let handle = self.spawn_fetch_page(1);
                    self.state = StreamState::FetchingPage(handle);
                }

                StreamState::FetchingPage(ref mut handle) => {
                    match Pin::new(handle).poll(cx) {
                        Poll::Ready(Ok(Ok(body))) => {
                            let (items, next_page) = self.extract_page_data(&body);

                            if items.is_empty() {
                                self.state = StreamState::Done;
                                return Poll::Ready(None);
                            }

                            self.current_page += 1;

                            // Start fetching next page immediately if it exists
                            if let Some(next) = next_page {
                                let next_fetch = self.spawn_fetch_page(next);
                                self.state = StreamState::FetchingNext {
                                    items,
                                    index: 0,
                                    next_fetch,
                                };
                            } else {
                                self.state = StreamState::YieldingItems {
                                    items,
                                    index: 0,
                                    next_page: None,
                                };
                            }
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
                    next_page,
                } => {
                    if *index < items.len() {
                        let item = items[*index].clone();
                        *index += 1;
                        return Poll::Ready(Some(Ok(item)));
                    } else {
                        // Page exhausted
                        if let Some(next) = *next_page {
                            let handle = self.spawn_fetch_page(next);
                            self.state = StreamState::FetchingPage(handle);
                        } else {
                            self.state = StreamState::Done;
                            return Poll::Ready(None);
                        }
                    }
                }

                StreamState::FetchingNext {
                    items,
                    index,
                    next_fetch,
                } => {
                    // Yield items while next page is fetching
                    if *index < items.len() {
                        let item = items[*index].clone();
                        *index += 1;
                        return Poll::Ready(Some(Ok(item)));
                    } else {
                        // Current page exhausted, check if next page is ready
                        match Pin::new(next_fetch).poll(cx) {
                            Poll::Ready(Ok(Ok(body))) => {
                                let (new_items, next_page) = self.extract_page_data(&body);

                                if new_items.is_empty() {
                                    self.state = StreamState::Done;
                                    return Poll::Ready(None);
                                }

                                self.current_page += 1;

                                // Start fetching the page after this one if it exists
                                if let Some(next) = next_page {
                                    let next_fetch = self.spawn_fetch_page(next);
                                    self.state = StreamState::FetchingNext {
                                        items: new_items,
                                        index: 0,
                                        next_fetch,
                                    };
                                } else {
                                    self.state = StreamState::YieldingItems {
                                        items: new_items,
                                        index: 0,
                                        next_page: None,
                                    };
                                }
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
                }

                StreamState::Done => return Poll::Ready(None),
            }
        }
    }
}
