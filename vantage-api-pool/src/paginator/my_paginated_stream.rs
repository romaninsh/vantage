use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{atomic::AtomicUsize, Arc},
    task::{Context, Poll},
};

use anyhow::Result;
use futures_core::Stream;
use serde_json::Value;
use tracing::Instrument as _;

use crate::AwwPool;

pub struct PageStream {
    endpoint: String,
    pool: Arc<AwwPool>,
    page: AtomicUsize,
    join_handles: VecDeque<tokio::task::JoinHandle<Result<Value>>>,
    total_pages: Option<usize>,
    page_fetch_limit: Option<usize>,
}

impl PageStream {
    pub fn new(pool: Arc<AwwPool>, endpoint: String) -> Self {
        let mut stream = Self {
            pool,
            endpoint,
            page: AtomicUsize::new(0),
            join_handles: VecDeque::new(),
            page_fetch_limit: None,
            total_pages: None,
        };
        stream.prefetch(1);
        stream
    }

    pub fn prefetch(&mut self, size: usize) {
        for _ in 0..size {
            let handle = self.spawn_fetch();
            self.join_handles.push_back(handle);
        }
    }

    pub fn set_max_prefetch(&mut self, pages: usize) {
        self.page_fetch_limit = Some(pages);
    }

    pub fn spawn_fetch(&mut self) -> tokio::task::JoinHandle<Result<Value>> {
        let pool = self.pool.clone();
        let endpoint = self.endpoint.clone();
        let page_num = self.page.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        tokio::spawn(
            async move {
                let path = format!("{}?page={}", endpoint, page_num + 1);
                let response = pool.get(&path).await?;
                let json = response.json::<Value>().await?;

                Ok(json)
            }
            .in_current_span(),
        )
    }
}

impl Stream for PageStream {
    type Item = Result<Value>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Check if we could be fetching some more pages right now
        if let Some(handle) = self.join_handles.front_mut() {
            match Pin::new(handle).poll(cx) {
                Poll::Ready(Ok(result)) => {
                    self.join_handles.pop_front();

                    // Update pagination state from response
                    if let Ok(ref json) = result {
                        if let Some(pagination) = json.get("pagination") {
                            // if we learn about total_pages
                            if let Some(total) =
                                pagination.get("total_pages").and_then(|v| v.as_u64())
                            {
                                if self.total_pages.is_none() {
                                    let total_pages = total as usize;
                                    let fetched_already =
                                        self.page.load(std::sync::atomic::Ordering::Relaxed);
                                    let mut will_prefetch = total_pages
                                        .saturating_sub(fetched_already)
                                        .saturating_sub(self.join_handles.len());
                                    if let Some(page_fetch_limit) = self.page_fetch_limit {
                                        will_prefetch = will_prefetch.min(
                                            page_fetch_limit
                                                .saturating_sub(self.join_handles.len()),
                                        );
                                    }

                                    self.total_pages = Some(total as usize);

                                    // Only prefetch if there are pages to fetch
                                    if will_prefetch > 0 {
                                        self.prefetch(will_prefetch);
                                    }
                                } else {
                                    // We already know total_pages, check if we should continue prefetching
                                    if let Some(page_fetch_limit) = self.page_fetch_limit {
                                        let total_pages = self.total_pages.unwrap();
                                        let fetched_already =
                                            self.page.load(std::sync::atomic::Ordering::Relaxed);
                                        let will_prefetch = page_fetch_limit
                                            .saturating_sub(self.join_handles.len())
                                            .min(
                                                total_pages
                                                    .saturating_sub(fetched_already)
                                                    .saturating_sub(self.join_handles.len()),
                                            );

                                        if will_prefetch > 0 {
                                            if let Some(has_next) =
                                                pagination.get("has_next").and_then(|v| v.as_bool())
                                            {
                                                if has_next {
                                                    self.prefetch(will_prefetch);
                                                }
                                            } else {
                                                self.prefetch(will_prefetch);
                                            }
                                        }
                                    } else {
                                        // No page limit set, prefetch remaining pages up to total
                                        let total_pages = self.total_pages.unwrap();
                                        let fetched_already =
                                            self.page.load(std::sync::atomic::Ordering::Relaxed);
                                        let will_prefetch = total_pages
                                            .saturating_sub(fetched_already)
                                            .saturating_sub(self.join_handles.len());

                                        if will_prefetch > 0 {
                                            self.prefetch(will_prefetch);
                                        }
                                    }
                                }
                            }
                        }
                    }

                    Poll::Ready(Some(result))
                }
                Poll::Ready(Err(join_error)) => {
                    self.join_handles.pop_front();
                    Poll::Ready(Some(Err(join_error.into())))
                }
                Poll::Pending => Poll::Pending,
            }
        } else {
            Poll::Ready(None)
        }
    }
}

pub struct ItemStream {
    page_stream: PageStream,
    buffered_items: VecDeque<Value>,
}

impl ItemStream {
    pub fn new(pool: Arc<AwwPool>, endpoint: String) -> Self {
        Self {
            page_stream: PageStream::new(pool, endpoint),
            buffered_items: VecDeque::new(),
        }
    }

    pub fn set_max_prefetch(&mut self, pages: usize) {
        self.page_stream.set_max_prefetch(pages)
    }
}

impl Stream for ItemStream {
    type Item = Result<Value>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(item) = self.buffered_items.pop_front() {
            return Poll::Ready(Some(Ok(item)));
        }

        match Pin::new(&mut self.page_stream).poll_next(cx) {
            Poll::Ready(Some(Ok(json))) => {
                if let Some(data) = json.get("data").and_then(|d| d.as_array()) {
                    self.buffered_items.extend(data.iter().cloned());

                    if let Some(item) = self.buffered_items.pop_front() {
                        Poll::Ready(Some(Ok(item)))
                    } else {
                        self.poll_next(cx)
                    }
                } else {
                    Poll::Ready(Some(Err(anyhow::anyhow!(
                        "Missing 'data' field in response"
                    ))))
                }
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}
