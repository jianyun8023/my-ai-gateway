//! Bounds the number of live streaming responses awaiting final accounting.
//! A permit is reserved before the response is returned and stays owned by the
//! response body until its completion callback moves it into the write task.
use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const MAX_OUTSTANDING_STREAMS: u32 = 64;
const MAX_CONCURRENT_WRITES: usize = 8;

#[derive(Clone)]
pub(crate) struct SettlementManager {
    inner: Arc<Inner>,
}

struct Inner {
    permits: Arc<Semaphore>,
    writes: Arc<Semaphore>,
    capacity: u32,
    closing: AtomicBool,
}

pub(crate) struct SettlementPermit {
    stream: OwnedSemaphorePermit,
    writes: Arc<Semaphore>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AdmissionError {
    Saturated,
    Closing,
}

impl Default for SettlementManager {
    fn default() -> Self {
        Self::with_capacity(MAX_OUTSTANDING_STREAMS)
    }
}

impl SettlementManager {
    pub(crate) fn with_capacity(capacity: u32) -> Self {
        Self::with_limits(capacity, MAX_CONCURRENT_WRITES)
    }

    fn with_limits(capacity: u32, writes: usize) -> Self {
        assert!(capacity > 0);
        assert!(writes > 0);
        Self {
            inner: Arc::new(Inner {
                permits: Arc::new(Semaphore::new(capacity as usize)),
                writes: Arc::new(Semaphore::new(writes)),
                capacity,
                closing: AtomicBool::new(false),
            }),
        }
    }

    pub(crate) fn try_reserve(&self) -> Result<SettlementPermit, AdmissionError> {
        if self.inner.closing.load(Ordering::Acquire) {
            return Err(AdmissionError::Closing);
        }
        let permit = self
            .inner
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| AdmissionError::Saturated)?;
        if self.inner.closing.load(Ordering::Acquire) {
            return Err(AdmissionError::Closing);
        }
        Ok(SettlementPermit {
            stream: permit,
            writes: self.inner.writes.clone(),
        })
    }

    /// Call only after the HTTP server has stopped accepting and draining
    /// response bodies. Acquiring all permits waits for every write task.
    pub(crate) async fn drain(&self) {
        self.inner.closing.store(true, Ordering::Release);
        let _all = self
            .inner
            .permits
            .acquire_many(self.inner.capacity)
            .await
            .expect("settlement semaphore is never closed");
    }
}

impl SettlementPermit {
    pub(crate) fn spawn<F>(self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(async move {
            let _stream = self.stream;
            let _write = self
                .writes
                .acquire()
                .await
                .expect("write semaphore is never closed");
            future.await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::atomic::AtomicUsize, time::Duration};
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn slow_write_saturates_and_drain_waits() {
        let manager = SettlementManager::with_capacity(1);
        let (release, wait) = oneshot::channel::<()>();
        manager.try_reserve().unwrap().spawn(async move {
            wait.await.unwrap();
        });
        assert!(matches!(
            manager.try_reserve(),
            Err(AdmissionError::Saturated)
        ));
        let drain = tokio::spawn({
            let manager = manager.clone();
            async move { manager.drain().await }
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(!drain.is_finished());
        release.send(()).unwrap();
        drain.await.unwrap();
        assert!(matches!(
            manager.try_reserve(),
            Err(AdmissionError::Closing)
        ));
    }

    #[tokio::test]
    async fn failed_write_releases_capacity() {
        let manager = SettlementManager::with_capacity(1);
        let completed = Arc::new(AtomicUsize::new(0));
        let marker = completed.clone();
        manager.try_reserve().unwrap().spawn(async move {
            let result: Result<(), &'static str> = Err("database unavailable");
            if result.is_err() {
                marker.fetch_add(1, Ordering::Relaxed);
            }
        });
        manager.drain().await;
        assert_eq!(completed.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn pending_writes_respect_the_write_limit() {
        let manager = SettlementManager::with_limits(2, 1);
        let entered = Arc::new(AtomicUsize::new(0));
        let (release, wait) = oneshot::channel::<()>();
        let marker = entered.clone();
        manager.try_reserve().unwrap().spawn(async move {
            marker.fetch_add(1, Ordering::Relaxed);
            wait.await.unwrap();
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while entered.load(Ordering::Relaxed) != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let marker = entered.clone();
        manager.try_reserve().unwrap().spawn(async move {
            marker.fetch_add(1, Ordering::Relaxed);
        });
        tokio::task::yield_now().await;
        assert_eq!(entered.load(Ordering::Relaxed), 1);
        assert!(matches!(
            manager.try_reserve(),
            Err(AdmissionError::Saturated)
        ));
        release.send(()).unwrap();
        manager.drain().await;
        assert_eq!(entered.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn dropped_response_body_still_runs_settlement() {
        let manager = SettlementManager::with_capacity(1);
        let permit = manager.try_reserve().unwrap();
        let completed = Arc::new(AtomicUsize::new(0));
        let marker = completed.clone();
        let body = crate::proxy::usage::observe_stream_body(
            axum::body::Body::empty(),
            std::time::Instant::now(),
            move |observation| {
                assert_eq!(
                    observation.termination,
                    crate::proxy::stream::StreamTermination::ClientCancelled
                );
                permit.spawn(async move {
                    marker.fetch_add(1, Ordering::Relaxed);
                });
            },
        );
        drop(body);
        manager.drain().await;
        assert_eq!(completed.load(Ordering::Relaxed), 1);
    }
}
