use super::{PrivateCacheLine, PrivateCachePokeResult, PrivateCacheSet};

struct HarvardPerCorePrivateCache<
    const I_SET: usize,
    const I_ASSO: usize,
    const D_SET: usize,
    const D_ASSO: usize,
> {
    i_cache: Box<[PrivateCacheSet<I_ASSO>; I_SET]>,
    d_cache: Box<[PrivateCacheSet<D_ASSO>; D_SET]>,
}
