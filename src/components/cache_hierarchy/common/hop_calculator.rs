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
