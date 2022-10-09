use std::sync::Mutex;

struct CacheSet {
    data: Vec<Option<u64>>,
    is_up: bool
}

impl CacheSet {
    pub fn 
}

pub struct Cache {
    block_size: u8,
    set: u64,
    associativity: u64,
    data: Vec<Mutex<CacheSet>>
}

impl Cache {
    pub fn new() -> Self {
        return Cache {
            block_size: todo!(),
            set: todo!(),
            associativity: todo!(),
            data: todo!(),
        };
    }
}

