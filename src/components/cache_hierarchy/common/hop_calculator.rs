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

// This file records a hop number calculator for a 2D mesh topology.
// It is based the X-Y / Y-X routing algorithm.

use crate::parameter::SIMULATED_CORE_COUNT;

// return (row, col)
const fn figure_out_mesh_size() -> (usize, usize) {
    match SIMULATED_CORE_COUNT {
        1 => (1, 1),
        2 => (1, 2),
        4 => (2, 2),
        16 => (4, 4),
        32 => (4, 8),
        48 => (6, 8),
        64 => (8, 8),
        128 => (8, 16),
        256 => (16, 16),
        1024 => (32, 32),
        _ => panic!("Unsupported core count for mesh topology."),
    }
}

const MESH_WIDTH: usize = figure_out_mesh_size().1;
#[cfg(debug_assertions)]
const MESH_HEIGHT: usize = figure_out_mesh_size().0;

#[inline(always)]
const fn map_core_to_coordinate(core_id: usize) -> (usize, usize) {
    let x = core_id % MESH_WIDTH;
    let y = core_id / MESH_WIDTH;
    #[cfg(debug_assertions)]
    if y >= MESH_HEIGHT {
        panic!("Core ID exceeds the number of cores in the mesh.");
    }
    (x, y)
}

#[inline(always)]
pub const fn calculate_hop_count(src_core_id: u32, dst_core_id: u32) -> usize {
    let (src_x, src_y) = map_core_to_coordinate(src_core_id as usize);
    let (dst_x, dst_y) = map_core_to_coordinate(dst_core_id as usize);
    let x_hops = if src_x > dst_x {
        src_x - dst_x
    } else {
        dst_x - src_x
    };
    let y_hops = if src_y > dst_y {
        src_y - dst_y
    } else {
        dst_y - src_y
    };
    x_hops + y_hops
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hop_count() {
        if SIMULATED_CORE_COUNT != 16 {
            println!(
                "Tests are designed for a 4x4 mesh (16 cores). Please set SIMULATED_CORE_COUNT to 16 for testing."
            );
            return;
        }
        // Test a few cases for a 4x4 mesh (16 cores)
        assert_eq!(calculate_hop_count(0, 15), 6); // (0,0) to (3,3)
        assert_eq!(calculate_hop_count(0, 5), 2); // (0,0) to (1,1)
        assert_eq!(calculate_hop_count(5, 0), 2); // (1,1) to (0,0)
    }
}
