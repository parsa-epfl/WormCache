use crate::{components::cache_hierarchy::hierarchy::CacheAccessType, parameter};
use std::fmt::Debug;

pub trait SharedCacheSetStatistics: Default + Debug {
    // print statistics
    fn get_header() -> String;
    fn render_line(&self) -> String;

    // record
    fn record(&mut self, access_type: CacheAccessType, is_os: bool, is_hit: bool);
}

#[derive(Debug)]
pub struct ZeroSharedCacheSetStatistics {}

impl Default for ZeroSharedCacheSetStatistics {
    fn default() -> Self {
        Self {}
    }
}

impl SharedCacheSetStatistics for ZeroSharedCacheSetStatistics {
    fn get_header() -> String {
        "".to_string()
    }

    fn render_line(&self) -> String {
        "".to_string()
    }

    fn record(&mut self, _access_type: CacheAccessType, _is_os: bool, _is_hit: bool) {}
}

#[derive(Debug)]
pub struct SharedCacheSetMissStatistics {
    // statistics
    pub miss_count: u64,
    pub miss_count_u: u64,
    pub miss_count_k: u64,

    pub fetch_miss_count: u64,
    pub fetch_miss_count_u: u64,
    pub fetch_miss_count_k: u64,

    pub read_miss_count: u64,
    pub read_miss_count_u: u64,
    pub read_miss_count_k: u64,

    pub write_miss_count: u64,
    pub write_miss_count_u: u64,
    pub write_miss_count_k: u64,

    pub ptw_miss_count: u64,
    pub ptw_miss_count_u: u64,
    pub ptw_miss_count_k: u64,
}

impl Default for SharedCacheSetMissStatistics {
    fn default() -> Self {
        Self {
            miss_count: 0,
            miss_count_u: 0,
            miss_count_k: 0,

            fetch_miss_count: 0,
            fetch_miss_count_u: 0,
            fetch_miss_count_k: 0,

            read_miss_count: 0,
            read_miss_count_u: 0,
            read_miss_count_k: 0,

            write_miss_count: 0,
            write_miss_count_u: 0,
            write_miss_count_k: 0,

            ptw_miss_count: 0,
            ptw_miss_count_u: 0,
            ptw_miss_count_k: 0,
        }
    }
}

impl SharedCacheSetStatistics for SharedCacheSetMissStatistics {
    fn get_header() -> String {
        let mut result = vec![];
        for basic_string in [
            "miss_count",
            "fetch_miss_count",
            "read_miss_count",
            "write_miss_count",
            "ptw_miss_count",
        ] {
            for suffix in ["", ":u", ":k"] {
                result.push(format!("{}{}", basic_string, suffix));
            }
        }

        result.join(",")
    }

    fn render_line(&self) -> String {
        let res = vec![
            self.miss_count.to_string(),
            self.miss_count_u.to_string(),
            self.miss_count_k.to_string(),
            self.fetch_miss_count.to_string(),
            self.fetch_miss_count_u.to_string(),
            self.fetch_miss_count_k.to_string(),
            self.read_miss_count.to_string(),
            self.read_miss_count_u.to_string(),
            self.read_miss_count_k.to_string(),
            self.write_miss_count.to_string(),
            self.write_miss_count_u.to_string(),
            self.write_miss_count_k.to_string(),
            self.ptw_miss_count.to_string(),
            self.ptw_miss_count_u.to_string(),
            self.ptw_miss_count_k.to_string(),
        ];

        res.join(",")
    }

    #[inline]
    fn record(&mut self, access_type: CacheAccessType, is_os: bool, _is_hit: bool) {
        if parameter::ENABLE_STATISTICS {
            self.miss_count += 1;

            match access_type {
                CacheAccessType::InstructionFetch => {
                    self.fetch_miss_count += 1;
                }
                CacheAccessType::DataRead => {
                    self.read_miss_count += 1;
                }
                CacheAccessType::DataWrite => {
                    self.write_miss_count += 1;
                }
                CacheAccessType::PageWalkRead => {
                    self.ptw_miss_count += 1;
                }

                _ => panic!("Error: unsupported access type."),
            }

            if is_os {
                self.miss_count_k += 1;
                match access_type {
                    CacheAccessType::InstructionFetch => {
                        self.fetch_miss_count_k += 1;
                    }
                    CacheAccessType::DataRead => {
                        self.read_miss_count_k += 1;
                    }
                    CacheAccessType::DataWrite => {
                        self.write_miss_count_k += 1;
                    }
                    CacheAccessType::PageWalkRead => {
                        self.ptw_miss_count_k += 1;
                    }
                    _ => panic!("Error: unsupported access type."),
                }
            } else {
                self.miss_count_u += 1;
                match access_type {
                    CacheAccessType::InstructionFetch => {
                        self.fetch_miss_count_u += 1;
                    }
                    CacheAccessType::DataRead => {
                        self.read_miss_count_u += 1;
                    }
                    CacheAccessType::DataWrite => {
                        self.write_miss_count_u += 1;
                    }
                    CacheAccessType::PageWalkRead => {
                        self.ptw_miss_count_u += 1;
                    }
                    _ => panic!("Error: unsupported access type."),
                }
            }
        }
    }
}
