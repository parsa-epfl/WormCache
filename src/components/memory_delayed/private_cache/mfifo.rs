use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    Invalidate,
    CreateSharer,
}

#[derive(Debug)]
pub struct FIFO<const SIZE: usize> {
    read_pointer: usize,
    buffer: UnsafeCell<[(u64, u64, MessageType); SIZE]>,
    write_pointer: AtomicUsize,
}

impl<const SIZE: usize> FIFO<SIZE> {
    pub fn new() -> Self {
        Self {
            buffer: UnsafeCell::new([(0, 0, MessageType::Invalidate); SIZE]),
            read_pointer: 0,
            write_pointer: AtomicUsize::new(0),
        }
    }

    pub fn push(&self, value: u64, ts: u64, message_type: MessageType) {
        let write_pointer = self.write_pointer.fetch_add(1, Ordering::Relaxed);
        // if not full, just write it. Otherwise, try to merge with the previous one.
        if write_pointer < self.read_pointer + SIZE {
            unsafe {
                (*self.buffer.get())[write_pointer % SIZE] = (value, ts, message_type);
            }
        } else {
            // search and see if there are any invalidate request pending.
            let mut found = false;
            for i in 0..SIZE {
                let index = (write_pointer - i) % SIZE;
                if unsafe { (*self.buffer.get())[index].0 == value } {
                    found = true;
                    break;
                }
            }

            if !found {
                // it is impassible to see this path.
                unreachable!("FIFO::push: the FIFO is full and no message can be combined.");
            }
        }
    }

    pub fn pop(&mut self) -> Option<(u64, u64, MessageType)> {
        if self.read_pointer == self.write_pointer.load(Ordering::Relaxed) {
            return None;
        } else {
            let res = unsafe { (*self.buffer.get())[self.read_pointer % SIZE] };
            self.read_pointer += 1;
            return Some(res);
        }
    }

    pub fn is_empty(&self) -> bool {
        return self.read_pointer == self.write_pointer.load(Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let fifo: FIFO<10> = FIFO::new();
        assert_eq!(fifo.read_pointer, 0);
        assert_eq!(fifo.write_pointer.load(Ordering::Relaxed), 0);
        assert_eq!(fifo.is_empty(), true);
    }

    #[test]
    fn test_push_and_pop() {
        let mut fifo: FIFO<10> = FIFO::new();
        fifo.push(1, 100, MessageType::Invalidate);
        assert_eq!(fifo.is_empty(), false);
        let res = fifo.pop();
        assert_eq!(res, Some((1, 100, MessageType::Invalidate)));
        assert_eq!(fifo.is_empty(), true);
    }

    #[test]
    fn test_push_and_pop_multiple() {
        let mut fifo: FIFO<10> = FIFO::new();
        for i in 0..10 {
            fifo.push(i, i * 100, MessageType::Invalidate);
        }
        assert_eq!(fifo.is_empty(), false);
        for i in 0..10 {
            let res = fifo.pop();
            assert_eq!(res, Some((i, i * 100, MessageType::Invalidate)));
        }
        assert_eq!(fifo.is_empty(), true);
    }

    #[test]
    #[should_panic(expected = "FIFO::push: the FIFO is full and no message can be combined.")]
    fn test_push_overflow() {
        let fifo: FIFO<10> = FIFO::new();
        for i in 0..11 {
            fifo.push(i, i * 100, MessageType::Invalidate);
        }
    }
}
