use worm_cache::{self, components::memory::{PrivateCacheParameters, CacheBlockState, TimestampMemoryHierarchy}};

macro_rules! access_cache {
    ($cache:expr, $ts:expr, $core_id:expr, $addr:expr, $is_write:expr) => {
        $ts += 1;
        $cache
            .hierarchies($core_id)
            .access_memory($ts, $addr, false, $is_write);
    };
}


const PARAM: PrivateCacheParameters = PrivateCacheParameters {
    l1i_sets: 1,
    l1i_associativity: 4,
    l1d_sets: 1,
    l1d_associativity: 4,
    l2_sets: 1,
    l2_associativity: 4,
    directory_associativity: 4,
};

// All cases to consider
// MOESI
// 
// Invalid:
// Invalid -> Exclusive
// Invalid -> Modified
// 
// Shared:
// Shared -> Invalid
// Shared -> Owned
// Shared -> Modified 
// 
// Exclusive:
// Exclusive -> Invalid (Other's writing)
// Exclusive -> Shared (Other's reading)
// Exclusive -> Modified (Self's writing)
//
// Modified:
// Modified -> Invalid (Other's writing)
// Modified -> Owned (Other's reading)
//
// Owned:
// Owned -> Invalid (Other's writing)

#[test]
fn invalid_to_exclusive() {
    let cache = TimestampMemoryHierarchy::<4, 1, 4, 1>::new(2);

    let mut ts = 0;

    // access order:
    // C0: read 0x00
    // C1: read 0x40 // next cache line
    // access_cache!(cache, ts, CORE_ID, ADDR, IS_WRITE)
    access_cache!(cache, ts, 0, 0, false);
    access_cache!(cache, ts, 1, 64, false);

    let mtr = cache.render_mtr::<1>();
    let p0 = mtr.render_private_caches(0, &PARAM);
    let p1 = mtr.render_private_caches(1, &PARAM);

    assert!(p0[1][0][0].state == CacheBlockState::CleanExclusive);
    assert!(p1[1][0][0].state == CacheBlockState::CleanExclusive);
}

#[test]
fn exclusive_to_shared() {
    let cache = TimestampMemoryHierarchy::<4, 1, 4, 1>::new(2);
    let mut ts = 0;

    // access order:
    // C0: read 0x0
    // C1: read 0x0
    // access_cache!(cache, ts, CORE_ID, ADDR, IS_WRITE)
    access_cache!(cache, ts, 0, 0, false);
    access_cache!(cache, ts, 1, 0, false);

    let mtr = cache.render_mtr::<1>();
    let p0 = mtr.render_private_caches(0, &PARAM);
    let p1 = mtr.render_private_caches(1, &PARAM);

    assert!(p0[1][0][0].state == CacheBlockState::CleanShared);
    assert!(p1[1][0][0].state == CacheBlockState::CleanShared);
}

#[test]
fn exclusive_to_modified() {
    let cache =TimestampMemoryHierarchy::<4, 1, 4, 1>::new(2);
    let mut ts = 0;

    // access logic:
    // C0: read 0x0
    // C0: write 0x0

    access_cache!(cache, ts, 0, 0, false);

    // reconstruction
    let mtr = cache.render_mtr::<1>();
    let p0 = mtr.render_private_caches(0, &PARAM);
    assert!(p0[1][0][0].state == CacheBlockState::CleanExclusive);

    access_cache!(cache, ts, 0, 0, true);
    let mtr = cache.render_mtr::<1>();
    let p0 = mtr.render_private_caches(0, &PARAM);
    assert!(p0[1][0][0].state == CacheBlockState::ModifiedExclusive);
}

#[test]
fn exclusive_to_owned() {
    // access logics:
    // C0: write 0x0
    // C1: read 0x0

    let cache = TimestampMemoryHierarchy::<4, 1, 4, 1>::new(2);
    let mut ts = 0;

    access_cache!(cache, ts, 0, 0, true);
    access_cache!(cache, ts, 1, 0, false);

    let mtr = cache.render_mtr::<1>();
    let p0 = mtr.render_private_caches(0, &PARAM);
    let p1 = mtr.render_private_caches(1, &PARAM);

    assert!(p0[1][0][0].state == CacheBlockState::ModifiedOwned);
    assert!(p1[1][0][0].state == CacheBlockState::CleanShared);
}

#[test]
fn shared_to_invalid_and_modified() {
    // access logics:
    // C0: read 0x0
    // C1: read 0x0
    // C0: write 0x0

    let cache = TimestampMemoryHierarchy::<4, 1, 4, 1>::new(2);
    let mut ts = 0;

    access_cache!(cache, ts, 0, 0, false);
    access_cache!(cache, ts, 1, 0, false);
    access_cache!(cache, ts, 0, 0, true);

    let mtr = cache.render_mtr::<1>();
    let p0 = mtr.render_private_caches(0, &PARAM);
    let p1 = mtr.render_private_caches(1, &PARAM);

    assert!(p0[1][0][0].state == CacheBlockState::ModifiedExclusive);
    assert!(p1[1][0][0].state == CacheBlockState::Invalid);
}

#[test]
fn shared_to_owned() {
    // access logics:
    // C0: read 0x0
    // C1: read 0x0
    // C0: write 0x0
    // C1: read 0x0

    let cache = TimestampMemoryHierarchy::<4, 1, 4, 1>::new(2);
    let mut ts = 0;

    access_cache!(cache, ts, 0, 0, false);
    access_cache!(cache, ts, 1, 0, false);
    access_cache!(cache, ts, 0, 0, true);
    access_cache!(cache, ts, 1, 0, false);

    let mtr = cache.render_mtr::<1>();
    let p0 = mtr.render_private_caches(0, &PARAM);
    let p1 = mtr.render_private_caches(1, &PARAM);

    assert!(p0[1][0][0].state == CacheBlockState::ModifiedOwned);
    assert!(p1[1][0][0].state == CacheBlockState::CleanShared);
}

