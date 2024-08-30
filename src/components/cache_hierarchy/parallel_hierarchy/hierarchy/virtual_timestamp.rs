// BSD 3-Clause License
//
// Copyright (c) 2024, Parallel Systems Architecture Laboratory (PARSA), EPFL.
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are met:
//
// 1. Redistributions of source code must retain the above copyright notice, this
//    list of conditions and the following disclaimer.
//
// 2. Redistributions in binary form must reproduce the above copyright notice,
//    this list of conditions and the following disclaimer in the documentation
//    and/or other materials provided with the distribution.
//
// 3. Neither the name of the PARSA, EPFL
//    nor the names of its contributors may be used to endorse or promote
//    products derived from this software without specific prior written
//    permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
// AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
// FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
// DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
// SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
// OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use crate::{
    components::{
        cache_hierarchy::common::{
            statistics::ZeroSharedCacheSetStatistics, CacheHierarchyAccessResult,
            ParallelSingleSharedCache, ParallelUnifiedPrivateCache,
        },
        debug::statistics::{EventType, Statistics},
        NoMMU,
    },
    parameter,
};

use super::{CacheAccessType, MemoryHierarchy};

const PCACHE_SET: usize = 64;

type MH = MemoryHierarchy<
    NoMMU,
    ParallelUnifiedPrivateCache<32, { PCACHE_SET }, { parameter::UNIFIED_PRI_CACHE_ASSO }>,
    ParallelSingleSharedCache<
        ZeroSharedCacheSetStatistics,
        { parameter::SHARED_CACHE_SET },
        { parameter::SHARED_CACHE_ASSO },
        { parameter::SHARED_CACHE_EXCLUSIVE },
    >,
    true,
    { parameter::SHARED_CACHE_FILL_WITH_PRIVATE_CACHE },
    { parameter::SHARED_CACHE_FILL_ON_CLEAN_EVICTION },
    { parameter::SHARED_CACHE_FILL_ON_DIRTY_EVICTION },
    { parameter::DIRECTORY_SHARD_COUNT },
>;

#[test]
#[ignore]
fn read_after_read_has_no_impact() {
    let mh = MH::new(true, 0, false);
    let block_id = 0x1234;
    let original_value =
        Statistics::global_query_record(1, EventType::PrivateCacheVTsOrderViolation).0;

    // first, core 0 reads, with ts = 10, v_ts = 2.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 10, 2, CacheAccessType::DataRead, false, 0),
        CacheHierarchyAccessResult::Miss
    );

    // then, core 1 reads, with ts = 20, and v_ts = 1.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 20, 1, CacheAccessType::DataRead, false, 0),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // there should be zero impact on the miss rate.
    assert_eq!(
        (Statistics::global_query_record(1, EventType::PrivateCacheVTsOrderViolation).0)
            - original_value,
        0
    );
}

#[test]
#[ignore]
fn read_after_write_has_impact() {
    let mh = MH::new(true, 0, false);
    let block_id = 0x1234;

    let original_value =
        Statistics::global_query_record(1, EventType::PrivateCacheVTsOrderViolation).0;

    // first, core 0 writes, with ts = 10, v_ts = 2.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 10, 2, CacheAccessType::DataWrite, false, 0),
        CacheHierarchyAccessResult::Miss
    );

    // To skip the write timestamp in the directory, we let core 0 write it again.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 15, 4, CacheAccessType::DataWrite, false, 0),
        CacheHierarchyAccessResult::HitInSelfPrivateCache
    );

    // then, core 1 reads, with ts = 20, and v_ts = 1.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 20, 3, CacheAccessType::DataRead, false, 0),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // there should be one impact on the miss rate.
    assert_eq!(
        (Statistics::global_query_record(1, EventType::PrivateCacheVTsOrderViolation).0)
            - original_value,
        1
    );
}

#[test]
#[ignore]
fn write_after_read_has_impact() {
    let mh = MH::new(true, 0, false);
    let block_id = 0x1234;

    let original_value =
        Statistics::global_query_record(1, EventType::PrivateCacheVTsOrderViolation).0;

    // first, core 0 reads, with ts = 10, v_ts = 2.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 10, 2, CacheAccessType::DataRead, false, 0),
        CacheHierarchyAccessResult::Miss
    );

    // then, core 1 writes, with ts = 20, and v_ts = 1.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 20, 1, CacheAccessType::DataWrite, false, 0),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // there should be one impact on the miss rate.
    assert_eq!(
        (Statistics::global_query_record(1, EventType::PrivateCacheVTsOrderViolation).0)
            - original_value,
        1
    );
}

#[test]
#[ignore]
fn write_after_write_has_impact() {
    let mh = MH::new(true, 0, false);
    let block_id = 0x1234;

    let original_value =
        Statistics::global_query_record(1, EventType::PrivateCacheVTsOrderViolation).0;

    // first, core 0 writes, with ts = 10, v_ts = 2.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 10, 2, CacheAccessType::DataWrite, false, 0),
        CacheHierarchyAccessResult::Miss
    );

    // To skip the write timestamp in the directory, we let core 0 write it again.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 15, 4, CacheAccessType::DataWrite, false, 0),
        CacheHierarchyAccessResult::HitInSelfPrivateCache
    );

    // then, core 1 writes, with ts = 20, and v_ts = 1.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 20, 3, CacheAccessType::DataWrite, false, 0),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // there should be one impact on the miss rate.
    assert_eq!(
        (Statistics::global_query_record(1, EventType::PrivateCacheVTsOrderViolation).0)
            - original_value,
        1
    );
}
