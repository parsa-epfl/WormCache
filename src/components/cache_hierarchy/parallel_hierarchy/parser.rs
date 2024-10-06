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

use crate::components::cache_hierarchy::mmu::MemoryManagementUnit;
/*
 * The purpose of this file is to provide a parser over the parameter.rs to generate the cache hierarchy at the compile time.
 *
 *
 * Basically, I want to implement the following logic:
 *
 * ```rust
 * type HierarchyForPlugin = match (SERIAL_CACHE_MODEL, UNIFIED_CACHE_MODEL) {
 *    (true, true) => ParallelMemoryHierarchyUnified,
 *    (true, false) => ParallelMemoryHierarchyHarvard,
 *    (false, true) => SerialMemoryHierarchyUnified,
 *    (false, false) => SerialMemoryHierarchyHarvard,
 * };
 *
 * Well, this is not available in the Rust. So, I have to use the trait and its polymorphism to implement the logic, which is extremely dirty.
 *
 * Reference: https://willcrichton.net/notes/type-level-programming/
 *
 */
use crate::parameter;
use crate::{arch::AArch64, components::cache_hierarchy::MemoryHierarchy};

pub trait CacheModelParser<const SERIAL_CACHE_MODEL: bool, const UNIFIED_CACHE_MODEL: bool> {
    type Output: MemoryHierarchy;
}

pub struct DummyParser;

pub trait SharedCacheStatisticsParser<const ENABLE_STATISTICS: bool> {
    type Output;
}

impl SharedCacheStatisticsParser<true> for DummyParser {
    type Output = SharedCacheSetMissStatistics;
}

impl SharedCacheStatisticsParser<false> for DummyParser {
    type Output = ZeroSharedCacheSetStatistics;
}

pub type SharedCacheStatisticsWithPlugin =
    <DummyParser as SharedCacheStatisticsParser<{ parameter::ENABLE_STATISTICS }>>::Output;

use super::{
    super::common::{
        statistics::{SharedCacheSetMissStatistics, ZeroSharedCacheSetStatistics},
        ParallelHarvardPrivateCache, ParallelSingleSharedCache, ParallelUnifiedPrivateCache,
        SerialHarvardPrivateCache, SerialSingleSharedCache, SerialUnifiedPrivateCache,
    },
    hierarchy,
};

pub const ALLOCATED_CORE_COUNT: usize = if parameter::MEASURE_HALF_OF_CORES {
    parameter::CORE_COUNT / 2
} else {
    parameter::CORE_COUNT
};

type AArch64MMU = MemoryManagementUnit<AArch64, { parameter::TLB_ASSO }, { parameter::TLB_SET }>;

#[allow(dead_code)]
type ParalleMemoryHierarchyUnified = hierarchy::ParallelMemoryHierarchy<
    AArch64MMU,
    ParallelUnifiedPrivateCache<
        { ALLOCATED_CORE_COUNT },
        { parameter::UNIFIED_PRI_CACHE_SET },
        { parameter::UNIFIED_PRI_CACHE_ASSO },
    >,
    ParallelSingleSharedCache<
        SharedCacheStatisticsWithPlugin,
        { parameter::SHARED_CACHE_SET },
        { parameter::SHARED_CACHE_ASSO },
        { parameter::SHARED_CACHE_EXCLUSIVE },
    >,
    { !parameter::DISABLE_PRECISE_COHERENCE_STATE_RECONSTRUCTION },
    { parameter::SHARED_CACHE_FILL_WITH_PRIVATE_CACHE },
    { parameter::SHARED_CACHE_FILL_ON_CLEAN_EVICTION },
    { parameter::SHARED_CACHE_FILL_ON_DIRTY_EVICTION },
    { parameter::DIRECTORY_SHARD_COUNT },
>;

#[allow(dead_code)]
type ParallelMemoryHierarchyHarvard = hierarchy::ParallelMemoryHierarchy<
    AArch64MMU,
    ParallelHarvardPrivateCache<
        { ALLOCATED_CORE_COUNT },
        { parameter::HARVARD_PRI_I_CACHE_SET },
        { parameter::HARVARD_PRI_I_CACHE_ASSO },
        { parameter::HARVARD_PRI_D_CACHE_SET },
        { parameter::HARVARD_PRI_D_CACHE_ASSO },
    >,
    ParallelSingleSharedCache<
        SharedCacheStatisticsWithPlugin,
        { parameter::SHARED_CACHE_SET },
        { parameter::SHARED_CACHE_ASSO },
        { parameter::SHARED_CACHE_EXCLUSIVE },
    >,
    { !parameter::DISABLE_PRECISE_COHERENCE_STATE_RECONSTRUCTION },
    { parameter::SHARED_CACHE_FILL_WITH_PRIVATE_CACHE },
    { parameter::SHARED_CACHE_FILL_ON_CLEAN_EVICTION },
    { parameter::SHARED_CACHE_FILL_ON_DIRTY_EVICTION },
    { parameter::DIRECTORY_SHARD_COUNT },
>;

#[allow(dead_code)]
type SerialMemoryHierarchyUnified = hierarchy::ParallelMemoryHierarchy<
    AArch64MMU,
    SerialUnifiedPrivateCache<
        { ALLOCATED_CORE_COUNT },
        { parameter::UNIFIED_PRI_CACHE_SET },
        { parameter::UNIFIED_PRI_CACHE_ASSO },
    >,
    SerialSingleSharedCache<
        SharedCacheStatisticsWithPlugin,
        { parameter::SHARED_CACHE_SET },
        { parameter::SHARED_CACHE_ASSO },
        { parameter::SHARED_CACHE_EXCLUSIVE },
    >,
    false,
    { parameter::SHARED_CACHE_FILL_WITH_PRIVATE_CACHE },
    { parameter::SHARED_CACHE_FILL_ON_CLEAN_EVICTION },
    { parameter::SHARED_CACHE_FILL_ON_DIRTY_EVICTION },
    { parameter::DIRECTORY_SHARD_COUNT },
>;

#[allow(dead_code)]
type SerialMemoryHierarchyHarvard = hierarchy::ParallelMemoryHierarchy<
    AArch64MMU,
    SerialHarvardPrivateCache<
        { ALLOCATED_CORE_COUNT },
        { parameter::HARVARD_PRI_I_CACHE_SET },
        { parameter::HARVARD_PRI_I_CACHE_ASSO },
        { parameter::HARVARD_PRI_D_CACHE_SET },
        { parameter::HARVARD_PRI_D_CACHE_ASSO },
    >,
    SerialSingleSharedCache<
        SharedCacheStatisticsWithPlugin,
        { parameter::SHARED_CACHE_SET },
        { parameter::SHARED_CACHE_ASSO },
        { parameter::SHARED_CACHE_EXCLUSIVE },
    >,
    false,
    { parameter::SHARED_CACHE_FILL_WITH_PRIVATE_CACHE },
    { parameter::SHARED_CACHE_FILL_ON_CLEAN_EVICTION },
    { parameter::SHARED_CACHE_FILL_ON_DIRTY_EVICTION },
    { parameter::DIRECTORY_SHARD_COUNT },
>;

impl CacheModelParser<true, true> for DummyParser {
    type Output = ParalleMemoryHierarchyUnified;
}

impl CacheModelParser<true, false> for DummyParser {
    type Output = ParallelMemoryHierarchyHarvard;
}

impl CacheModelParser<false, true> for DummyParser {
    type Output = SerialMemoryHierarchyUnified;
}

impl CacheModelParser<false, false> for DummyParser {
    type Output = SerialMemoryHierarchyHarvard;
}

pub type HierarchyForPlugin = <DummyParser as CacheModelParser<
    { !parameter::USE_SERIAL_CACHE_MODEL },
    { parameter::USE_UNIFIED_CACHE },
>>::Output;
