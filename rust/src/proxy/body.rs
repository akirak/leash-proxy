//! A body wrapper that counts transferred bytes for audit logging.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};

use bytes::Bytes;
use http_body::{Body, Frame, SizeHint};
use pin_project_lite::pin_project;

/// Shared byte counter.
pub type SharedCounter = Arc<AtomicU64>;

/// Callback fired exactly once with the total number of data bytes yielded
/// by a body.
pub type OnFinish = Box<dyn FnOnce(u64) + Send + Sync + 'static>;

/// Guarantees a callback runs exactly once: either when the body completes
/// (end-of-stream or error) or no later than when the body is dropped.
///
/// The drop case matters because HTTP stacks are free to never poll a body
/// they know is empty — without this, the audit record for such responses
/// would never be emitted.
pub struct FinishOnDrop {
    on_finish: Option<OnFinish>,
    counter: SharedCounter,
}

impl FinishOnDrop {
    fn none() -> Self {
        Self {
            on_finish: None,
            counter: Arc::new(AtomicU64::new(0)),
        }
    }

    fn new(on_finish: OnFinish, counter: SharedCounter) -> Self {
        Self {
            on_finish: Some(on_finish),
            counter,
        }
    }

    fn fire(&mut self) {
        if let Some(on_finish) = self.on_finish.take() {
            on_finish(self.counter.load(Ordering::Relaxed));
        }
    }
}

impl Drop for FinishOnDrop {
    fn drop(&mut self) {
        self.fire();
    }
}

pin_project! {
    /// Wraps a body, counting every data byte it yields to the consumer.
    ///
    /// Used to report `bytes_in`/`bytes_out` in audit records without
    /// buffering request or response bodies.
    pub struct CountedBody<B> {
        #[pin]
        inner: B,
        counter: SharedCounter,
        finish: FinishOnDrop,
    }
}

impl<B> CountedBody<B> {
    /// Wrap `inner` so every data byte read is added to `counter`.
    pub fn counting(inner: B, counter: SharedCounter) -> Self {
        Self {
            inner,
            counter,
            finish: FinishOnDrop::none(),
        }
    }

    /// Like [`CountedBody::counting`], but invoke `on_finish` exactly once
    /// with the total byte count when the body completes — whether it
    /// reached end-of-stream, returned an error, or was dropped without
    /// being fully polled.
    pub fn finishing(inner: B, counter: SharedCounter, on_finish: OnFinish) -> Self {
        Self {
            inner,
            counter: counter.clone(),
            finish: FinishOnDrop::new(on_finish, counter),
        }
    }
}

impl<B> Body for CountedBody<B>
where
    B: Body<Data = Bytes>,
{
    type Data = Bytes;
    type Error = B::Error;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Bytes>, Self::Error>>> {
        let this = self.project();
        let result = this.inner.poll_frame(cx);
        match result {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    this.counter.fetch_add(data.len() as u64, Ordering::Relaxed);
                }
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(err))) => {
                this.finish.fire();
                Poll::Ready(Some(Err(err)))
            }
            Poll::Ready(None) => {
                this.finish.fire();
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use http_body_util::Full;
    use std::sync::atomic::AtomicU64;

    #[tokio::test]
    async fn counts_bytes_and_fires_on_finish() {
        let counter: SharedCounter = Arc::new(AtomicU64::new(0));
        let body = Full::new(Bytes::from_static(b"hello world"));
        let body = CountedBody::finishing(
            body,
            counter.clone(),
            Box::new(|total| assert_eq!(total, 11)),
        );
        let collected = body.collect().await.unwrap().to_bytes();
        assert_eq!(collected.len(), 11);
        assert_eq!(counter.load(Ordering::Relaxed), 11);
    }

    #[tokio::test]
    async fn empty_body_fires_on_finish() {
        let counter: SharedCounter = Arc::new(AtomicU64::new(0));
        let body = Full::new(Bytes::new());
        let body = CountedBody::finishing(body, counter, Box::new(|total| assert_eq!(total, 0)));
        body.collect().await.unwrap().to_bytes();
    }

    #[tokio::test]
    async fn fires_on_drop_without_polling() {
        // A body that is dropped without being polled to completion must
        // still fire the callback.
        let counter: SharedCounter = Arc::new(AtomicU64::new(0));
        let body = Full::new(Bytes::from_static(b"dropped"));
        let counted = CountedBody::finishing(body, counter, Box::new(|total| assert_eq!(total, 0)));
        drop(counted);
    }

    #[tokio::test]
    async fn counting_only_never_fires() {
        let counter: SharedCounter = Arc::new(AtomicU64::new(0));
        let body = Full::new(Bytes::from_static(b"one"));
        let counted = CountedBody::counting(body, counter.clone());
        drop(counted);
        assert_eq!(counter.load(Ordering::Relaxed), 0);
    }
}
