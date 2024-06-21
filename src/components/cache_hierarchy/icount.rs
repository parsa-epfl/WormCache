use std::cell::UnsafeCell;

use crate::parameter::CORE_COUNT;

#[repr(align(64))]
#[derive(Debug, Clone, Copy)]
pub struct PerCoreICount {
    icount: u64,
    last_icount: u64,
}

impl PerCoreICount {
    pub fn new() -> PerCoreICount {
        PerCoreICount {
            icount: 0,
            last_icount: 0,
        }
    }

    pub fn reset(&mut self) {
        self.icount = 0;
    }
}

impl Default for PerCoreICount {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct ICountPlugin {
    data: [UnsafeCell<PerCoreICount>; CORE_COUNT],
}

impl ICountPlugin {
    pub fn new() -> ICountPlugin {
        ICountPlugin {
            data: std::array::from_fn(|_| UnsafeCell::new(PerCoreICount::new())),
        }
    }

    pub fn get_icount(&self, core_id: u8) -> u64 {
        unsafe { (*self.data[core_id as usize as usize].get()).icount }
    }

    pub fn increase_icount(&self, core_id: u8, icount: u64) {
        unsafe {
            let core_id = core_id as usize;
            (*self.data[core_id].get()).icount += (*self.data[core_id].get()).last_icount;
            (*self.data[core_id].get()).last_icount = icount;

            // the reason why we do so is because this function is called before instructions are actually executed.
            // therefore, when getting the icount, the instruction should not see the icount of the current translation block.
        }
    }
}

impl Default for ICountPlugin {
    fn default() -> Self {
        Self::new()
    }
}
