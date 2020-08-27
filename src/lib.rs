use tokio::sync::Mutex;
use std::collections::VecDeque;
use std::future::Future;
use std::task::{Context, Waker};
use tokio::macros::support::{Pin, Poll};
use std::mem;
use std::sync::Arc;

struct PoolInner<T: 'static + Default + Send> {
    objects: Vec<T>,
    pending: VecDeque<Waker>,
}

pub struct Pool<T: 'static + Default + Send> {
    inner: Arc<Mutex<PoolInner<T>>>,
}

pub struct PoolFuture<'a, T: 'static + Default + Send> {
    in_pool: &'a Pool<T>
}

pub struct PoolGuard<'a, T: 'static + Default + Send> {
    content: T,
    in_pool: &'a Pool<T>,
}

impl<'a, T: 'static + Default + Send> Drop for PoolGuard<'a, T> {
    fn drop(&mut self) {
        let content = mem::take(&mut self.content);
        let inner = self.in_pool.inner.clone();
        tokio::spawn(async move {
            let mut inner = inner.lock().await;
            inner.objects.push(content);
            inner.pending.pop_front().map(|it| it.wake());
        });
    }
}

impl<'a, T: 'static + Default + Send> Future for PoolFuture<'a, T> {
    type Output = PoolGuard<'a, T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Ok(mut inner) = self.in_pool.inner.try_lock() {
            if inner.objects.is_empty() {
                inner.pending.push_back(cx.waker().clone());
                Poll::Pending
            } else {
                Poll::Ready(PoolGuard {
                    content: inner.objects.pop().unwrap(),
                    in_pool: self.in_pool,
                })
            }
        } else {
            Poll::Pending
        }
    }
}

impl<T: 'static + Default + Send> Pool<T> {
    pub fn new(size: usize, mut constructor: impl FnMut() -> T) -> Self {
        Self {
            inner: Arc::new(Mutex::new(PoolInner {
                objects: (0..size).map(|_| constructor()).collect(),
                pending: Default::default(),
            })),
        }
    }

    pub fn take(&self) -> PoolFuture<T> {
        PoolFuture {
            in_pool: self
        }
    }

    pub fn try_take(&self) -> Option<PoolGuard<T>> {
        let mut inner = self.inner.try_lock().ok()?;
        inner.objects.pop()
            .map(|content| {
                PoolGuard {
                    content,
                    in_pool: self,
                }
            })
    }

    pub fn take_or(&self, value: T) -> PoolGuard<T> {
        self.try_take().unwrap_or_else(|| PoolGuard {
            content: value,
            in_pool: self,
        })
    }

    pub fn take_or_default(&self) -> PoolGuard<T> {
        self.take_or(Default::default())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
