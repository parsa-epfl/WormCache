use crate::CORE_COUNT;

#[derive(Debug)]
pub struct ICountPlugin {
    data: [u64; CORE_COUNT * 8],
}

impl ICountPlugin {
    pub fn get_icounts(&self) -> [u64; CORE_COUNT] {
        let mut res = [0; CORE_COUNT];
        for i in 0..CORE_COUNT {
            res[i] = self.data[i * 8];
        }
        return res;
    }

    pub fn increase_icount(&self, core_id: u8, count: usize) {
        // DMN, some dirty technology should be used.
        // I guarantee that there is no contention.
        unsafe {
            let pos = &self.data[core_id as usize * 8] as *const u64 as *mut u64;
            // volatile load and store is necessary, because the compiler will optimize the code.
            pos.write_volatile(pos.read_volatile() + count as u64);
        }
    }

    pub fn new() -> ICountPlugin {
        return ICountPlugin {
            data: [0; CORE_COUNT * 8],
        };
    }

    pub fn reset(&self) {
        // Still dirty. You must make sure that when doing reset, there is no other threads.
        unsafe {
            for i in 0..CORE_COUNT {
                let ptr = self.data.as_ptr().add((i * 8) as usize);
                let ptr = ptr as *mut u64;
                *ptr = 0;
            }
        }
    }
}
