//! Allocation-on-demand queue shared by Win32 hosts during callback reentrancy.

use std::{cell::RefCell, collections::VecDeque};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueueError {
    Borrowed,
    Full,
}

pub(crate) struct BoundedQueue<T, const CAPACITY: usize> {
    items: RefCell<VecDeque<T>>,
}

impl<T, const CAPACITY: usize> BoundedQueue<T, CAPACITY> {
    pub(crate) const fn new() -> Self {
        Self {
            items: RefCell::new(VecDeque::new()),
        }
    }

    pub(crate) fn push(&self, item: T) -> Result<(), QueueError> {
        let mut items = self
            .items
            .try_borrow_mut()
            .map_err(|_| QueueError::Borrowed)?;
        if items.len() >= CAPACITY {
            return Err(QueueError::Full);
        }
        items.push_back(item);
        Ok(())
    }

    #[cfg_attr(not(feature = "native-pixel-window"), allow(dead_code))]
    pub(crate) fn push_front(&self, item: T) -> Result<(), QueueError> {
        let mut items = self
            .items
            .try_borrow_mut()
            .map_err(|_| QueueError::Borrowed)?;
        if items.len() >= CAPACITY {
            return Err(QueueError::Full);
        }
        items.push_front(item);
        Ok(())
    }

    pub(crate) fn pop(&self) -> Result<Option<T>, QueueError> {
        self.items
            .try_borrow_mut()
            .map(|mut items| items.pop_front())
            .map_err(|_| QueueError::Borrowed)
    }

    #[cfg_attr(not(feature = "native-pixel-window"), allow(dead_code))]
    pub(crate) fn is_empty(&self) -> Result<bool, QueueError> {
        self.items
            .try_borrow()
            .map(|items| items.is_empty())
            .map_err(|_| QueueError::Borrowed)
    }

    pub(crate) fn clear(&self) -> Result<(), QueueError> {
        self.items
            .try_borrow_mut()
            .map(|mut items| items.clear())
            .map_err(|_| QueueError::Borrowed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_is_fifo_and_full_does_not_drop_existing_items() {
        let queue = BoundedQueue::<u8, 2>::new();
        queue.push(1).expect("first");
        queue.push(2).expect("second");
        assert_eq!(queue.push(3), Err(QueueError::Full));
        assert_eq!(queue.pop(), Ok(Some(1)));
        assert_eq!(queue.pop(), Ok(Some(2)));
        assert_eq!(queue.pop(), Ok(None));
    }

    #[test]
    fn queue_reports_reentrant_internal_borrow() {
        let queue = BoundedQueue::<u8, 1>::new();
        let _borrow = queue.items.borrow_mut();
        assert_eq!(queue.push(1), Err(QueueError::Borrowed));
        assert_eq!(queue.pop(), Err(QueueError::Borrowed));
    }
}
