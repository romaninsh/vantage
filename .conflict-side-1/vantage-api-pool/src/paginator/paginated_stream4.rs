use std::{
    collections::VecDeque,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use anyhow::Result;
use futures_core::Stream;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::AwwPool;

pub struct PageStream {
    item_receiver: mpsc::UnboundedReceiver<Result<Value>>,
    _worker_handle: tokio::task::JoinHandle<()>,
}

impl PageStream {
    pub fn new(pool: Arc<AwwPool>, endpoint: String, prefetch_limit: Option<usize>) -> Self {
        let (item_sender, item_receiver) = mpsc::unbounded_channel();

        let worker_handle = tokio::spawn(async move {
            Self::worker_thread(pool, endpoint, prefetch_limit, item_sender).await;
        });

        Self {
            item_receiver,
            _worker_handle: worker_handle,
        }
    }

    async fn worker_thread(
        pool: Arc<AwwPool>,
        endpoint: String,
        prefetch_limit: Option<usize>,
        item_sender: mpsc::UnboundedSender<Result<Value>>,
    ) {
        // Fetch first page to get pagination info
        let first_response = match pool.get(&format!("{}?page=1", endpoint)).await {
            Ok(resp) => resp,
            Err(e) => {
                let _ = item_sender.send(Err(e));
                return;
            }
        };

        let first_json: Value = match first_response.json().await {
            Ok(json) => json,
            Err(e) => {
                let _ = item_sender.send(Err(e.into()));
                return;
            }
        };

        // Extract pagination info
        let total_pages = first_json
            .get("pagination")
            .and_then(|p| p.get("total_pages"))
            .and_then(|t| t.as_u64())
            .unwrap_or(1) as usize;

        // Send items from first page
        if let Some(data) = first_json.get("data").and_then(|d| d.as_array()) {
            for item in data {
                if item_sender.send(Ok(item.clone())).is_err() {
                    return; // Receiver dropped
                }
            }
        }

        // If only one page, we're done
        if total_pages <= 1 {
            return;
        }

        // Fetch all remaining pages (prefetch_limit should control concurrency, not total pages)
        let remaining_pages = total_pages - 1;

        if remaining_pages == 0 {
            return; // No more pages to fetch
        }

        // Control concurrency with prefetch_limit, but fetch all pages
        let max_concurrent = prefetch_limit.unwrap_or(10);
        let mut join_handles = VecDeque::new();
        let mut pages_spawned = 0;
        let mut next_page = 2;

        // Spawn initial batch up to concurrency limit
        while pages_spawned < max_concurrent && next_page <= total_pages {
            let pool = pool.clone();
            let endpoint = endpoint.clone();
            let page = next_page;
            let handle = tokio::spawn(async move {
                let response = pool.get(&format!("{}?page={}", endpoint, page)).await?;
                let json: Value = response.json().await?;
                Ok((page, json))
            });
            join_handles.push_back(handle);
            pages_spawned += 1;
            next_page += 1;
        }

        // Process all completed tasks
        while let Some(handle) = join_handles.pop_front() {
            match handle.await {
                Ok(Ok((_page, json))) => {
                    // Send items from this page
                    if let Some(data) = json.get("data").and_then(|d| d.as_array()) {
                        for item in data {
                            if item_sender.send(Ok(item.clone())).is_err() {
                                return; // Receiver dropped
                            }
                        }
                    }

                    // Spawn next page if available
                    if next_page <= total_pages {
                        let pool = pool.clone();
                        let endpoint = endpoint.clone();
                        let page = next_page;
                        let handle = tokio::spawn(async move {
                            let response = pool.get(&format!("{}?page={}", endpoint, page)).await?;
                            let json: Value = response.json().await?;
                            Ok((page, json))
                        });
                        join_handles.push_back(handle);
                        next_page += 1;
                    }
                }
                Ok(Err(e)) => {
                    let _ = item_sender.send(Err(e));
                    return;
                }
                Err(e) => {
                    let _ = item_sender.send(Err(e.into()));
                    return;
                }
            }
        }
    }
}

impl Stream for PageStream {
    type Item = Result<Value>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.item_receiver.poll_recv(cx) {
            Poll::Ready(Some(item)) => Poll::Ready(Some(item)),
            Poll::Ready(None) => Poll::Ready(None), // Channel closed
            Poll::Pending => Poll::Pending,
        }
    }
}

pub struct ItemStream {
    page_stream: PageStream,
    buffered_items: VecDeque<Value>,
}

impl ItemStream {
    pub fn new(pool: Arc<AwwPool>, endpoint: String, prefetch_limit: Option<usize>) -> Self {
        Self {
            page_stream: PageStream::new(pool, endpoint, prefetch_limit),
            buffered_items: VecDeque::new(),
        }
    }
}

impl Stream for ItemStream {
    type Item = Result<Value>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(item) = self.buffered_items.pop_front() {
            return Poll::Ready(Some(Ok(item)));
        }

        match Pin::new(&mut self.page_stream).poll_next(cx) {
            Poll::Ready(Some(Ok(item))) => Poll::Ready(Some(Ok(item))),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}
