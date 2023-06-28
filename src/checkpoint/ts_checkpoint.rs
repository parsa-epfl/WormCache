use std::collections::BinaryHeap;

use super::{CacheBlock, DirectoryBlock};

// There might be cases when you need the timestamp of cache block to achieve a different replacement policy.
pub trait TimestampedBlock {
    type ExportedType;

    fn ts(&self) -> usize;
    fn export(self) -> Self::ExportedType;
}


#[derive(PartialEq, Eq, Clone)]
pub struct TsCacheBlock {
    pub d: CacheBlock,
    pub ts: usize
}

impl TimestampedBlock for TsCacheBlock {
    type ExportedType = CacheBlock;

    fn ts(&self) -> usize {
        return self.ts;
    }

    fn export(self) -> Self::ExportedType {
        return self.d;
    }
}


/// TODO: Replace the following code with a macro.
impl Ord for TsCacheBlock {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        return self.ts().cmp(&other.ts());
    }
}

impl PartialOrd for TsCacheBlock {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        return Some(self.cmp(other));
    }
}

#[derive(PartialEq, Eq, Debug)]

pub struct TsDirectoryBlock {
    pub d: DirectoryBlock,
    pub ts: usize
}

impl TimestampedBlock for TsDirectoryBlock {
    type ExportedType = DirectoryBlock;

    fn ts(&self) -> usize {
        return self.ts;
    }

    fn export(self) -> Self::ExportedType {
        return self.d;
    }
}

impl Ord for TsDirectoryBlock {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        return self.ts().cmp(&other.ts());
    }
}

impl PartialOrd for TsDirectoryBlock {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        return Some(self.cmp(other));
    }
}


/// Heap for ordering the timestamped block. It will automatic sort elements when appending.
pub struct TsOrderingHeap<T: TimestampedBlock + Ord> {
    d: BinaryHeap<T>
}

impl<T: TimestampedBlock + Ord> TsOrderingHeap<T> {

    pub fn new() -> Self {
        return Self {
            d: BinaryHeap::new()
        };
    }

    pub fn from_binary_heap(heap: BinaryHeap<T>) -> Self {
        return Self {
            d: heap
        };
    }

    pub fn push(&mut self, another: T) {
        self.d.push(another);
    }

    pub fn keep_top_n(mut self, n: usize) -> Vec<T> {
        if n > self.d.len() {
            let mut res = Vec::with_capacity(self.d.len());
            for _ in 0..self.d.len() {
                res.push(self.d.pop().unwrap());
            }
            return res;
        } else {
            let mut res = Vec::with_capacity(n);
            for _ in 0..n {
                res.push(self.d.pop().unwrap());
            }
            return res;
        }
    }
}

#[test]
fn test_ts_ordering_heap() {
    let mut simple = TsOrderingHeap::<TsDirectoryBlock>::new();

    impl TsDirectoryBlock {
        fn with_directory_block(block: DirectoryBlock, ts: usize) -> Self {
            return Self {
                d: block,
                ts
            }
        }
    }

    simple.push(TsDirectoryBlock::with_directory_block(DirectoryBlock {
        block_id: 0x100000,
        replicas: vec![],
        last_writer: None,
    }, 10));

    simple.push(TsDirectoryBlock::with_directory_block(DirectoryBlock {
        block_id: 0x200000,
        replicas: vec![],
        last_writer: None,
    }, 12));


    simple.push(TsDirectoryBlock::with_directory_block(DirectoryBlock {
        block_id: 0x200000,
        replicas: vec![],
        last_writer: None,
    }, 8));

    simple.push(TsDirectoryBlock::with_directory_block(DirectoryBlock {
        block_id: 0x400000,
        replicas: vec![],
        last_writer: None,
    }, 2));


    println!("{:?}", simple.keep_top_n(3));

}