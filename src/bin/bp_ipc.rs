use worm_cache::components::{
    bp::{fetch::FetchUnit, BranchResolutionResult, BranchType},
    debug::statistics::Statistics,
};

use rand::prelude::*;

type FetchUnitType = FetchUnit<1>;

/*
 * What to test?
 *
 * There are three components in the FetchUnit:
 * 1. btb: btb::BTB<{ parameter::BTB_SET }, { parameter::BTB_ASSO }>
 * 2. ras: ras::ReturnAddressStacle<BP_RAS_COUNT>
 * 3. tage: tage::TAGEPredictor
 *
 * BTB is similar to a cache, so its extreme case is either a hit or a miss.
 * RAS should have the same speed whenever it is a hit or a miss.
 * TAGE has two branches inside:
 * - Whether its prediction is correct (it requires an allocation, allocation)
 * - Whether the prediction is from the btable or the gtable
 *
 *  It is really hard to
 *
*/

#[allow(dead_code)]
fn all_hits_btb() {
    let mut fetch_unit = FetchUnitType::new();
    let indirect_branch_result = BranchResolutionResult {
        branch_type: BranchType::IndirectBranch,
        is_taken: true,
    };

    fetch_unit.train(0, 4, indirect_branch_result, 8);

    for _ in 0..(1000 * 1000 * 1000) {
        fetch_unit.train(0, 4, indirect_branch_result, 8);
    }

    println!("{}", Statistics::global_one_line_statistics());
}

#[allow(dead_code)]
fn all_misses_btb_same_branch() {
    let mut fetch_unit = FetchUnitType::new();
    let indirect_branch_result = BranchResolutionResult {
        branch_type: BranchType::IndirectBranch,
        is_taken: true,
    };

    fetch_unit.train(0, 4, indirect_branch_result, 8);

    for idx in 0..(1000 * 1000) {
        fetch_unit.train(0, 4, indirect_branch_result, 8 + idx * 4);
    }

    println!("{}", Statistics::global_one_line_statistics());
}

#[allow(dead_code)]
fn all_misses_btb_different_branches() {
    // This test can trigger the slow path of the BTB due to the eviction.
    let mut fetch_unit = FetchUnitType::new();
    let indirect_branch_result = BranchResolutionResult {
        branch_type: BranchType::IndirectBranch,
        is_taken: true,
    };

    for pc in 0..(1000 * 1000) {
        let pc = pc * worm_cache::parameter::BTB_SET as u64 + 8;
        fetch_unit.train(0, pc, indirect_branch_result, pc * 4);
    }

    println!("{}", Statistics::global_one_line_statistics());
}

#[allow(dead_code)]
fn exercise_ras_best() {
    let mut fetch_unit = FetchUnitType::new();
    // it should be a pair of call and the return.
    let caller_pc = 0x4;
    let callee_pc = 0x0;

    let call_result = BranchResolutionResult {
        branch_type: BranchType::DirectCall,
        is_taken: true,
    };

    let return_result = BranchResolutionResult {
        branch_type: BranchType::Return,
        is_taken: true,
    };

    for idx in 0..(1000 * 1000) {
        let is_call = idx % 2 == 0;
        if is_call {
            fetch_unit.train(0, caller_pc, call_result, callee_pc);
        } else {
            fetch_unit.train(0, callee_pc, return_result, caller_pc + 4);
        }
    }

    println!("{}", Statistics::global_one_line_statistics());
}

#[allow(dead_code)]
fn tage_best_case() {
    let mut fetch_unit = FetchUnitType::new();

    let taken = BranchResolutionResult {
        branch_type: BranchType::Conditional,
        is_taken: true,
    };

    for _ in 0..(100 * 1000 * 1000) {
        fetch_unit.train(0, 4, taken, 16);
    }

    println!("{}", Statistics::global_one_line_statistics());
}

#[allow(dead_code)]
fn tage_worst_case() {
    // generate 1000 branches, and their directions are random.
    let branches = (0..100 * 1000)
        .map(|_| {
            let pc = random::<u64>();
            let direction = random::<bool>();
            (pc, direction)
        })
        .collect::<Vec<_>>();

    let mut fetch_unit = FetchUnitType::new();

    let taken = BranchResolutionResult {
        branch_type: BranchType::Conditional,
        is_taken: true,
    };

    let not_taken = BranchResolutionResult {
        branch_type: BranchType::Conditional,
        is_taken: false,
    };

    for _ in 0..1000 {
        for (pc, direction) in branches.iter() {
            fetch_unit.train(0, *pc, if *direction { taken } else { not_taken }, 0);
        }
    }

    println!("{}", Statistics::global_one_line_statistics());
}

fn main() {
    // all_hits_btb();
    // all_misses_btb_same_branch();
    // all_misses_btb_different_branches();
    // exercise_ras_best();
    // tage_worst_case();
    tage_best_case();
}
