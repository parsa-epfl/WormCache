use super::directory;
use super::parallel;

pub struct ParallelCacheHierarchyWithoutL2<const CORE_COUNT: usize> {
    l1is: [parallel::ParallelCache; CORE_COUNT],
    shared: parallel::ParallelCache,
    directory: directory::Directory,
}

impl<const CORE_COUNT: usize> ParallelCacheHierarchyWithoutL2<CORE_COUNT> {
    pub fn new(l1_set: usize, l1_asso: usize, llc_set: usize, llc_asso: usize) -> Self {
        return ParallelCacheHierarchyWithoutL2 {
            l1is: std::array::from_fn(|_| return parallel::ParallelCache::new(l1_set, l1_asso)),
            shared: parallel::ParallelCache::new(llc_set, llc_asso),
            directory: directory::Directory::new(),
        };
    }
    pub fn access(&self, addr: usize, is_store: bool, core_id: u8) {
        let perm = if is_store {
            parallel::BlockState::Modified
        } else {
            parallel::BlockState::Shared
        };
        match self.l1is[core_id as usize].update(addr >> 6, perm) {
            super::CacheReturnResult::Miss => todo!(),
            super::CacheReturnResult::Hit => todo!(),
            super::CacheReturnResult::MissWithEviction(_) => todo!(),
            super::CacheReturnResult::MissWithDirtyEviction(_) => todo!(),
            super::CacheReturnResult::MissWithWrongPermission => todo!(),
        }

    }
}
