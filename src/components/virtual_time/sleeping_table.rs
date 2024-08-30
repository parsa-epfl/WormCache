use std::cell::UnsafeCell;

#[derive(Debug)]
#[repr(align(64))]
struct IsSleeping {
    is_sleeping: bool,
}

pub struct SleepingTable<const CORE_COUNT: usize> {
    is_sleeping: [UnsafeCell<IsSleeping>; CORE_COUNT],
}

impl<const CORE_COUNT: usize> SleepingTable<CORE_COUNT> {
    pub fn new() -> Self {
        Self {
            is_sleeping: std::array::from_fn(|_| {
                UnsafeCell::new(IsSleeping { is_sleeping: false })
            }),
        }
    }

    pub fn set_sleeping(&self, core_id: usize, is_sleeping: bool) {
        // self.is_sleeping[core_id].is_sleeping = is_sleeping;
        unsafe {
            (*self.is_sleeping[core_id].get()).is_sleeping = is_sleeping;
        }
    }

    pub fn has_slept(&self, core_id: usize) -> bool {
        unsafe { (*self.is_sleeping[core_id].get()).is_sleeping }
    }

    pub fn clean_sleeping(&self) {
        for i in 0..CORE_COUNT {
            self.set_sleeping(i, false);
        }
    }
}
