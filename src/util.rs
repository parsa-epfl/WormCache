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

use std::cell::RefCell;
use std::collections::HashMap;

use bitvec::array::BitArray;
use bitvec::order::Lsb0;

thread_local! {
    static COMPRESSOR: RefCell<zstd::bulk::Compressor<'static>> =
        RefCell::new(zstd::bulk::Compressor::new(0).unwrap());
}

pub fn write_compressed(path: &str, bytes: &[u8]) {
    use std::io::Write;
    let mut file = std::fs::File::create(path).unwrap();
    COMPRESSOR.with(|c| {
        let compressed = c.borrow_mut().compress(bytes).unwrap();
        file.write_all(&compressed).unwrap();
    });
}

pub fn read_compressed(path: &str) -> Vec<u8> {
    use std::io::Read;
    let file = std::fs::File::open(path).unwrap();
    let mut decoder = zstd::stream::read::Decoder::new(file).unwrap();
    let mut bytes = Vec::new();
    decoder.read_to_end(&mut bytes).unwrap();
    bytes
}

/// Rkyv wrapper for BitArray that serializes/deserializes the underlying storage as Vec<u64>.
pub struct RkyvBitArray;

impl<const N: usize> rkyv::with::ArchiveWith<BitArray<[u64; N], Lsb0>> for RkyvBitArray {
    type Archived = rkyv::Archived<Vec<u64>>;
    type Resolver = rkyv::Resolver<Vec<u64>>;

    fn resolve_with(
        field: &BitArray<[u64; N], Lsb0>,
        resolver: Self::Resolver,
        out: rkyv::Place<Self::Archived>,
    ) {
        let data: Vec<u64> = field.as_raw_slice().to_vec();
        rkyv::Archive::resolve(&data, resolver, out);
    }
}

impl<const N: usize, S> rkyv::with::SerializeWith<BitArray<[u64; N], Lsb0>, S> for RkyvBitArray
where
    S: rkyv::ser::Allocator + rkyv::ser::Writer + rkyv::rancor::Fallible + ?Sized,
    S::Error: rkyv::rancor::Source,
{
    fn serialize_with(
        field: &BitArray<[u64; N], Lsb0>,
        serializer: &mut S,
    ) -> Result<Self::Resolver, S::Error> {
        let data: Vec<u64> = field.as_raw_slice().to_vec();
        rkyv::Serialize::serialize(&data, serializer)
    }
}

impl<const N: usize, D>
    rkyv::with::DeserializeWith<rkyv::Archived<Vec<u64>>, BitArray<[u64; N], Lsb0>, D>
    for RkyvBitArray
where
    D: rkyv::rancor::Fallible + ?Sized,
    D::Error: rkyv::rancor::Source,
{
    fn deserialize_with(
        field: &rkyv::Archived<Vec<u64>>,
        deserializer: &mut D,
    ) -> Result<BitArray<[u64; N], Lsb0>, D::Error> {
        let data: Vec<u64> = rkyv::Deserialize::deserialize(field, deserializer)?;
        let array: [u64; N] = data.try_into().expect("BitArray size mismatch");
        Ok(BitArray::new(array))
    }
}

// Return value: Map[first_index, size]
pub fn find_fetch_block_from_block_id_sequence(i: Vec<usize>) -> HashMap<usize, usize> {
    let mut res = HashMap::new();
    let mut last_fb_block_id: Option<usize> = None;
    let mut last_fb_first_instruction_index: usize = 0;
    let total = i.len();

    // traverse the array and find the inconsistent position.
    for (idx, block_id) in i.into_iter().enumerate() {
        match last_fb_block_id {
            Some(last_fb_id) => {
                if block_id != last_fb_id {
                    // We start a new block.
                    // First, keep the old block.
                    let last_fb_size = idx - last_fb_first_instruction_index;
                    res.insert(last_fb_first_instruction_index, last_fb_size);
                    // Then, adjust the information of the last block
                    last_fb_first_instruction_index = idx;
                    last_fb_block_id = Some(block_id);
                }
            }
            None => {
                last_fb_block_id = Some(block_id);
            }
        }
    }

    // insert the last element as well.
    res.insert(
        last_fb_first_instruction_index,
        total - last_fb_first_instruction_index,
    );

    res
}

pub fn init_heap_array<T: Sized + std::fmt::Debug, const N: usize>(
    f: impl Fn(usize) -> T,
) -> Box<[T; N]> {
    let res = Vec::from_iter((0..N).map(f));
    res.into_boxed_slice().try_into().unwrap()
}

use libc::{CLOCK_MONOTONIC_RAW, clock_gettime, timespec};

pub fn get_monotonic_ts() -> u64 {
    let mut ts = timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        assert!(clock_gettime(CLOCK_MONOTONIC_RAW, &mut ts) == 0);
    }
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

#[test]
fn test_get_monotonic_ts() {
    let ts = (0..100).map(|_| get_monotonic_ts()).collect::<Vec<u64>>();
    for i in 0..ts.len() - 1 {
        assert!(ts[i] < ts[i + 1]);
    }
}

#[test]
fn test_find_fetch_block_from_pa_sequence() {
    let example = vec![0, 0, 1, 1, 2, 2, 3, 3, 3];
    let res = find_fetch_block_from_block_id_sequence(example);
    assert_eq!(res.len(), 4);
    assert_eq!(res.get(&0), Some(&2));
    assert_eq!(res.get(&2), Some(&2));
    assert_eq!(res.get(&4), Some(&2));
    assert_eq!(res.get(&6), Some(&3));

    let example = vec![0];
    let res = find_fetch_block_from_block_id_sequence(example);
    assert_eq!(res.len(), 1);
    assert_eq!(res.get(&0), Some(&1));

    let example = vec![1];
    let res = find_fetch_block_from_block_id_sequence(example);
    assert_eq!(res.len(), 1);
    assert_eq!(res.get(&0), Some(&1));

    let example = vec![1, 2, 2, 10, 10];
    let res = find_fetch_block_from_block_id_sequence(example);
    assert_eq!(res.len(), 3);
    assert_eq!(res.get(&0), Some(&1));
    assert_eq!(res.get(&1), Some(&2));
    assert_eq!(res.get(&3), Some(&2));
}
