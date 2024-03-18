use super::{PrivateCacheLine, PrivateCachePokeResult, PrivateCacheSet};
use std::sync::Mutex;

#[repr(align(64))]
pub struct HarvardPerCorePrivateCache<
    const I_SET: usize,
    const I_ASSO: usize,
    const D_SET: usize,
    const D_ASSO: usize,
> {
    i_cache: Box<[Mutex<PrivateCacheSet<I_ASSO>>; I_SET]>,
    d_cache: Box<[Mutex<PrivateCacheSet<D_ASSO>>; D_SET]>,
}

pub struct HarvardPrivateCache {}
