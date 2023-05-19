use crate::QEMUPlugin;
use std::collections::HashSet;
use std::io::prelude::*;
use std::io::BufWriter;

use super::QEMUMemoryInfo;
use super::QEMUPluginBasicBlock;

// Associativity is defined as a constant in order to enable the optimization from the compiler.
#[derive(Debug)]

pub struct WarmupLatencyCacheLine<const ASSO: usize> {
    data: HashSet<usize>,
}

impl<const N: usize> WarmupLatencyCacheLine<N> {
    pub fn update(&mut self, addr: usize) -> bool {
        if self.data.len() < N {
            self.data.insert(addr);
            if self.data.len() == N {
                return true; // just warm up
            }
        }
        return false;
    }

    pub fn is_full(&self) -> bool {
        return self.data.len() >= N;
    }

    pub fn element_count(&self) -> usize {
        return self.data.len();
    }
}

#[derive(Debug)]
pub struct WarmupLatencyCache<const ASSO: usize, const SET_COUNT: usize> {
    body: Vec<WarmupLatencyCacheLine<ASSO>>,
    warmed_count: usize,
    report_counter: usize,
}

impl<const ASSO: usize, const SET_COUNT: usize> WarmupLatencyCache<ASSO, SET_COUNT> {
    pub fn new() -> Self {
        return WarmupLatencyCache {
            body: Vec::from(
                (0..SET_COUNT)
                    .map(|i| {
                        return WarmupLatencyCacheLine::<ASSO> {
                            data: HashSet::new(),
                        };
                    })
                    .collect::<Vec<WarmupLatencyCacheLine<ASSO>>>(),
            ),
            warmed_count: 0,
            report_counter: 0,
        };
    }

    pub fn update(&mut self, addr: usize, increase_counter: bool) -> bool {
        let set_id = (addr >> 6) % SET_COUNT;
        let res = self.body[set_id].update(addr >> 6);
        if res {
            self.warmed_count += 1;
        }

        if increase_counter {
            self.report_counter += 1;
        }

        // if self.report_counter % (1024 * 1024) == 0 {
        //     // report the warm up count.
        //     println!("[WormCache]: {}, {}", self.report_counter, self.warmed_count);
        // }

        return res;
    }

    pub fn is_warmed(&self) -> bool {
        return self
            .body
            .iter()
            .map(WarmupLatencyCacheLine::<ASSO>::is_full)
            .reduce(|x, y| -> bool {
                return x && y;
            })
            .unwrap();
    }

    pub fn current_warmup_count(&self) -> usize {
        return self.warmed_count;
    }

    pub fn current_instruction_count(&self) -> usize {
        return self.report_counter;
    }

    pub fn current_usage(&self) -> f64 {
        return (self
            .body
            .iter()
            .map(|x| -> usize { x.element_count() })
            .sum::<usize>() as f64)
            / (ASSO * SET_COUNT) as f64;
    }
}

impl<const ASSO: usize, const SET_COUNT: usize> Default for WarmupLatencyCache<ASSO, SET_COUNT> {
    fn default() -> Self {
        Self::new()
    }
}

const SET_COUNT: usize = 2048 * 1024;

pub struct FirstTouchCounterPlugin {
    table: WarmupLatencyCache<16, SET_COUNT>,
    log_file: BufWriter<std::fs::File>,
}

impl FirstTouchCounterPlugin {
    pub fn new() -> Self {
        let res = FirstTouchCounterPlugin {
            table: WarmupLatencyCache::<16, SET_COUNT>::new(),
            log_file: BufWriter::new(std::fs::File::create("touch_cache.log").unwrap()),
        };
        return res;
    }

    pub fn current_usage(&self) -> f64 {
        return self.table.current_usage();
    }
}

unsafe impl QEMUPlugin for FirstTouchCounterPlugin {
    unsafe fn on_translation(&mut self, tb: &QEMUPluginBasicBlock) -> Vec<*mut std::ffi::c_void> {
        return tb
            .iter()
            .map(|x| {
                return x.physical_address() as *mut std::ffi::c_void;
            })
            .collect();
    }

    unsafe fn on_instruction_execution(&mut self, cpu_idx: u32, user_data: *mut std::ffi::c_void) {
        let hva = user_data as usize;
        if cpu_idx == 1 {
            if self.table.update(hva, true) {
                // write down the content
                self.log_file
                    .write_fmt(format_args!(
                        "{}, {} \n",
                        self.table.current_instruction_count(),
                        self.table.current_warmup_count()
                    ))
                    .unwrap();
            }
        }
    }

    unsafe fn on_memory_access(
        &mut self,
        cpu_idx: u32,
        info: &QEMUMemoryInfo,
        vaddr: u64,
        user_data: *mut std::ffi::c_void,
    ) {
        if cpu_idx == 1 {
            if let Some(hva) = info.translate(vaddr) {
                if self.table.update(hva as usize, false) {
                    self.log_file
                        .write_fmt(format_args!(
                            "{}, {} \n",
                            self.table.current_instruction_count(),
                            self.table.current_warmup_count()
                        ))
                        .unwrap();
                }
            }
        }
    }

    unsafe fn on_qemu_exit(&mut self) {
        self.log_file.flush().unwrap();
    }
}

mod test {
    #[test]
    fn functionality() {
        let mut cache_body = super::WarmupLatencyCache::<8, 2>::new();
        for i in 0..(1024 * 1024 + 1) {
            cache_body.update((i % 8) * 64 * 2, true);
        }
        for i in 0..(1024 * 1024 + 1) {
            cache_body.update((i % 8) * 64 * 2 + 64, true);
        }
        println!("Warmed:{}", cache_body.is_warmed());
    }

    #[test]
    fn access_to_single_block() {
        let mut cache = super::WarmupLatencyCache::<2, 8>::new();
        assert!(cache.update(0, true) != true);
        assert!(cache.update(1, true) != true);
    }
}
