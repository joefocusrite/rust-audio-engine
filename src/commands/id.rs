use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Id(usize);

impl Id {
    pub fn with_value(value: usize) -> Self {
        Self(value)
    }

    pub fn generate() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}
