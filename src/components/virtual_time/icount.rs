use std::cell::UnsafeCell;

use crate::parameter as param;
use param::CORE_COUNT;

#[repr(align(64))]
#[derive(Debug, Clone, Copy)]
pub struct PerCoreICount {
    user_icount: u64,
    kernel_icount: u64,
}

impl PerCoreICount {
    pub fn new() -> PerCoreICount {
        return PerCoreICount {
            user_icount: 0,
            kernel_icount: 0,
        };
    }

    pub fn sum(&self) -> u64 {
        return self.user_icount + self.kernel_icount;
    }

    pub fn reset(&mut self) {
        self.user_icount = 0;
        self.kernel_icount = 0;
    }
}

#[derive(Debug)]
pub struct ICountPlugin {
    data: [UnsafeCell<PerCoreICount>; CORE_COUNT],
}

impl ICountPlugin {
    pub fn get_icounts(&self) -> [(u64, u64); CORE_COUNT] { // (user_icount, kernel_icount)
        let mut res = [(0, 0); CORE_COUNT];
        for i in 0..CORE_COUNT {
            unsafe {
                res[i] = (
                    (*self.data[i].get()).user_icount,
                    (*self.data[i].get()).kernel_icount,
                );
            }
        }
        return res;
    }

    pub fn get_total_icounts_of_core(&self, core_id: u8) -> u64 {
        unsafe {
            let core_id = core_id as usize;
            return (*self.data[core_id].get()).sum();
        }
    }

    pub fn increase_user_icount(&self, core_id: u8, icount: u64) {
        unsafe {
            let core_id = core_id as usize;
            (*self.data[core_id].get()).user_icount += icount;
        }
    }

    pub fn increase_kernel_icount(&self, core_id: u8, icount: u64) {
        unsafe {
            let core_id = core_id as usize;
            (*self.data[core_id].get()).kernel_icount += icount;
        }
    }

    pub fn new() -> ICountPlugin {
        return ICountPlugin {
            data: std::array::from_fn(|_| UnsafeCell::new(PerCoreICount::new())),
        };
    }

    pub fn reset(&self) {
        // Still dirty. You must make sure that when doing reset, there is no other threads.
        unsafe {
            for i in 0..CORE_COUNT {
                (*self.data[i].get()).reset();
            }
        }
    }
}
