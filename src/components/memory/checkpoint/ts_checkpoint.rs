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

pub trait LRUPrioritizing {
    type ExportedItem;
    fn get_top_k(self, k: usize) -> Self;

    fn export(self) -> Vec<Self::ExportedItem>;
}

impl LRUPrioritizing for BinaryHeap<TsCacheBlock> {
    type ExportedItem = CacheBlock;

    #[inline]
    fn get_top_k(self, k: usize) -> Self {
        if k >= self.len() {
            return self;
        }
        let mut copy = self;
        let mut res = BinaryHeap::with_capacity(k);

        for _ in 0..k {
            res.push(copy.pop().unwrap());
        }

        return res;
    }

    #[inline]
    fn export(self) -> Vec<Self::ExportedItem> {
        let mut copy = self;
        let mut res = Vec::with_capacity(copy.len());
        for _ in 0..copy.len() {
            res.push(copy.pop().unwrap().d)
        }
        return res;
    }
}



