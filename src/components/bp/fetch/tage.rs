#![allow(dead_code)]

// This file contains the basic TAGE branch predictor.
// It is basically an one-to-one translation of the C++ implementation in QFlex.

use crate::components::bp::BranchResolveFlag;

use serde::{Deserialize, Serialize};

// bits per counter in the global history tables
const CBITS: usize = 3;

// the default predictor
// by default a 63.5  Kbits predictor, featuring 7 tagged components and a base bimodal component:
// NHIST = 7, LOGB =13, LOGG=9, CBITS=3
// 10 Kbits for the bimodal table.
// 8.5 Kbits for T0
// 8 Kbits  for T1 and T2
// 7.5 Kbits for T3 and T4
// 7 Kbits for T5 and T6

const LOGB: usize = 13;
const NHIST: usize = 7;
// base 2 logarithm of number of entries  on each tagged component
const LOGG: usize = LOGB - 4;

// Total width of an entry in the tagged table with the longest history length
const TBITS: usize = 12;

// AS: we use Geometric history length
// AS: maximum global history length used and minimum history length
const MAXHIST: usize = 131;
const MINHIST: usize = 5;

type Address = u64;

type History = [bool; MAXHIST];

#[derive(Debug, Serialize, Deserialize)]
struct FoldedHistory {
    comp: u32,
    c_length: u32,
    o_length: u32,
    out_point: u32,
}

impl FoldedHistory {
    fn new() -> FoldedHistory {
        FoldedHistory {
            comp: 0,
            c_length: 0,
            o_length: 0,
            out_point: 0,
        }
    }

    fn init(&mut self, original_length: u32, compressed_length: u32) {
        self.comp = 0;
        self.o_length = original_length;
        self.c_length = compressed_length;
        self.out_point = self.o_length % self.c_length;
        assert!(self.o_length < MAXHIST as u32); // MAXHIST needs to be defined somewhere
    }

    // I am very curious why only h[0] and h[o_length] are used.
    fn update(&mut self, h: &History) {
        assert!((self.comp >> self.c_length) == 0);
        self.comp = (self.comp << 1) | (h[0] as u32);
        self.comp ^= (h[self.o_length as usize] as u32) << self.out_point;
        self.comp ^= self.comp >> self.c_length;
        self.comp &= (1 << self.c_length) - 1;
    }
}

// bimodal table entry
#[derive(Debug, Serialize, Deserialize)]
struct TAGEBiModalEntry {
    hyst: i8,
    pred: i8,
}

impl TAGEBiModalEntry {
    fn new() -> TAGEBiModalEntry {
        TAGEBiModalEntry { hyst: 1, pred: 0 }
    }
}

// global table entry
#[derive(Debug, Serialize, Deserialize)]
struct TAGEGlobalTableEntry {
    ctr: i8,
    tag: u16,
    ubit: i8,
}

impl TAGEGlobalTableEntry {
    fn new() -> TAGEGlobalTableEntry {
        TAGEGlobalTableEntry {
            ctr: 0,
            tag: 0,
            ubit: 0,
        }
    }
}

enum PredictionResult {
    StronglyTaken,
    Taken,
    NotTaken,
    StronglyNotTaken,
}

struct TAGEPredictionResultWithBank {
    pub result: bool,
    pub bank: usize,
    pub alternate_prediction: bool,
    pub gi: Vec<usize>,
    pub bi: usize,
}

#[derive(Debug)]
pub struct TAGEPredictor {
    // pwin: i32,

    // 4 bits to determine whether newly allocated entries should be considered as
    // valid or not for delivering  the prediction
    tick: i32,
    // path history, 16 entries.
    phist: i32,
    // phist_runahead: i32,
    // phist_retired: i32,

    // use a path history as for the OGEHL predictor
    ghist: History,
    // ghist_runahead: History,
    // ghist_retired: History,
    ch_i: [FoldedHistory; NHIST],
    ch_t: [[FoldedHistory; NHIST]; 2],
    // ch_i_runahead: [FoldedHistory; NHIST],
    // ch_t_runahead: [[FoldedHistory; NHIST]; 2],
    btable: Box<[TAGEBiModalEntry; 1 << LOGB]>,
    gtable: [Box<[TAGEGlobalTableEntry; 1 << LOGG]>; NHIST],

    // used for storing the history lengths
    m: [usize; NHIST], // this value stores the length of histories to combine to access each table.

    // the seed for pseudo-random number generator
    seed: i32,
}

impl TAGEPredictor {
    pub fn new() -> TAGEPredictor {
        // interpolate values between [MINHIST, MAXHIST-1] using geometric series
        let mut m = [0; NHIST];
        m[0] = MAXHIST - 1;
        m[NHIST - 1] = MINHIST;
        for i in 1..(NHIST - 1) {
            let base = (MAXHIST - 1) as f64 / MINHIST as f64;
            let exp = i as f64 / (NHIST - 1) as f64;
            m[NHIST - 1 - i] = (MINHIST as f64 * f64::powf(base, exp)).ceil() as usize;
        }

        println!("m: {:?}", m);

        TAGEPredictor {
            seed: 0,
            tick: 0,

            phist: 0,
            // phist_runahead: 0,
            // phist_retired: 0,
            ghist: [false; MAXHIST],
            // ghist_runahead: [false; MAXHIST],
            // ghist_retired: [false; MAXHIST],
            ch_i: std::array::from_fn(|idx| {
                let mut fh = FoldedHistory::new();
                fh.init(m[idx] as u32, LOGG as u32);
                fh
            }),

            // ch_i_runahead: std::array::from_fn(|idx| {
            //     let mut fh = FoldedHistory::new();
            //     fh.init(m[idx] as u32, LOGG as u32);
            //     fh
            // }),
            ch_t: [
                std::array::from_fn(|idx| {
                    let mut fh = FoldedHistory::new();
                    fh.init(m[idx] as u32, (TBITS - ((idx + (NHIST & 1)) / 2)) as u32);
                    fh
                }),
                std::array::from_fn(|idx| {
                    let mut fh = FoldedHistory::new();
                    fh.init(
                        m[idx] as u32,
                        (TBITS - ((idx + (NHIST & 1)) / 2) - 1) as u32,
                    );
                    fh
                }),
            ],

            // ch_t_runahead: [
            //     std::array::from_fn(|idx| {
            //         let mut fh = FoldedHistory::new();
            //         fh.init(m[idx] as u32, (TBITS - ((idx + (NHIST & 1)) / 2)) as u32);
            //         fh
            //     }),
            //     std::array::from_fn(|idx| {
            //         let mut fh = FoldedHistory::new();
            //         fh.init(
            //             m[idx] as u32,
            //             (TBITS - ((idx + (NHIST & 1)) / 2) - 1) as u32,
            //         );
            //         fh
            //     }),
            // ],
            btable: Box::new(std::array::from_fn(|_| TAGEBiModalEntry::new())),
            gtable: std::array::from_fn(|_| {
                Box::new(std::array::from_fn(|_| TAGEGlobalTableEntry::new()))
            }),

            m,
        }
    }

    fn bindex(&self, pc: Address) -> usize {
        let b_mask = (1 << LOGB) - 1;
        (pc & b_mask) as usize
    }

    // I am really confused by this function.
    fn gindex(&self, pc: Address, bank: usize) -> usize {
        let path_history_mixer_hash_function = |path_history: u32, size: usize, bank: usize| {
            let a = (path_history as usize) & ((1 << size) - 1);
            let a1 = a & ((1 << LOGG) - 1);
            let a2 = a >> LOGG;
            let a2 = (a2 << bank) & (((1 << LOGG) - 1) + (a2 >> (LOGG - bank)));
            let a = a1 ^ a2;

            (a << bank) & (((1 << LOGG) - 1) + (a >> (LOGG - bank)))
        };

        let index_without_path =
            pc ^ (pc >> (LOGG - NHIST + bank + 1)) ^ self.ch_i[bank].comp as u64;

        let index = if self.m[bank] >= 16 {
            index_without_path
                ^ path_history_mixer_hash_function(self.phist as u32, 16, bank) as u64
        } else {
            index_without_path
                ^ path_history_mixer_hash_function(self.phist as u32, self.m[bank], bank) as u64
        };

        let g_mask = (1 << LOGG) - 1;

        (index & g_mask) as usize
    }

    fn gtag(&self, pc: Address, bank: usize) -> u16 {
        let tag = pc ^ self.ch_t[0][bank].comp as u64 ^ (self.ch_t[1][bank].comp << 1) as u64;
        let mask = (1 << (TBITS - (bank + (NHIST & 1)) / 2)) - 1;
        (tag & mask) as u16
    }

    fn ctrupdate(cnt: i8, taken: bool, nbits: usize) -> i8 {
        let max: i8 = (1 << (nbits - 1)) - 1;
        let min: i8 = -max - 1;
        if taken {
            if cnt < max {
                cnt + 1
            } else {
                cnt
            }
        } else if cnt > min {
            cnt - 1
        } else {
            cnt
        }
    }

    fn is_cond_taken(&self, pc: Address) -> TAGEPredictionResultWithBank {
        let pc = pc >> 2; // pc is always aligned to 4 bytes
        let bi: usize = self.bindex(pc);
        let gi: Vec<_> = (0..NHIST).map(|idx| self.gindex(pc, idx)).collect();

        let mut which_bank = NHIST;
        let mut alter_which_bank: usize = NHIST;

        for idx in 0..NHIST {
            if self.gtable[idx][gi[idx]].tag == self.gtag(pc, idx) {
                // it is a hit!
                which_bank = idx;
                break;
            }
        }

        for idx in (which_bank + 1)..NHIST {
            if self.gtable[idx][gi[idx]].tag == self.gtag(pc, idx) {
                // it is a hit!
                alter_which_bank = idx;
                break;
            }
        }

        if which_bank < NHIST {
            // get the alter_prediction result
            let alternate_prediction = if alter_which_bank < NHIST {
                self.gtable[alter_which_bank][gi[alter_which_bank]].ctr >= 0
            } else {
                self.btable[bi].pred > 0
            };
            let cnt = self.gtable[which_bank][gi[which_bank]].ctr;
            TAGEPredictionResultWithBank {
                result: cnt >= 0,
                bank: which_bank,
                alternate_prediction,
                gi,
                bi,
            }
        } else {
            let alternate_prediction = self.btable[bi].pred > 0;
            TAGEPredictionResultWithBank {
                result: alternate_prediction,
                bank: which_bank,
                alternate_prediction,
                gi,
                bi,
            }
        }
    }

    fn shift_global_history(&mut self, taken: bool) {
        self.ghist.rotate_right(1);
        self.ghist[0] = taken;
    }

    fn update_history(&mut self, pc: Address, taken: bool) {
        // update ghist.
        self.shift_global_history(taken);
        // update phist.
        self.phist = (self.phist << 1) | ((pc >> 2) & 1) as i32;
        self.phist &= (1 << 16) - 1;

        // update ch_i
        for idx in 0..NHIST {
            self.ch_i[idx].update(&self.ghist);
            self.ch_t[0][idx].update(&self.ghist);
            self.ch_t[1][idx].update(&self.ghist);
        }
    }

    fn get_random(&mut self) -> i32 {
        self.seed = ((1 << (2 * NHIST)) + 1) * self.seed + 0xf3f531;
        self.seed &= (1 << (2 * (NHIST))) - 1;
        self.seed
    }

    pub fn train(
        &mut self,
        pc: u64,
        result: BranchResolveFlag,
        _target: u64,
    ) -> BranchPredictorResult {
        // we only update the predictor when the branch is conditional, but we update the history all the time.
        let is_conditional =
            result == BranchResolveFlag::Taken || result == BranchResolveFlag::NotTaken;
        let taken = result == BranchResolveFlag::Taken;
        if is_conditional {
            let pc = pc >> 2;
            let prediction_result = self.is_cond_taken(pc);
            let allocation = prediction_result.result != taken;

            if allocation {
                let mut min: i8 = 3; // the the minimum useful counter value
                for idx in 0..(prediction_result.bank) {
                    if self.gtable[idx][prediction_result.gi[idx]].ubit < min {
                        min = self.gtable[idx][prediction_result.gi[idx]].ubit;
                    }
                }

                if min > 0 {
                    // NO UNUSEFUL ENTRY TO ALLOCATE: age all possible targets, but do not allocate
                    for idx in 0..(prediction_result.bank) {
                        self.gtable[idx][prediction_result.gi[idx]].ubit -= 1;
                    }
                } else {
                    // YES: allocate one entry, but apply some randomness
                    // bank I is twice more probable than bank I-1
                    let n_rand = self.get_random();
                    let mut y = n_rand & ((1 << (prediction_result.bank - 1)) - 1);
                    let mut x = prediction_result.bank - 1;
                    while (y & 1) != 0 {
                        x -= 1;
                        y >>= 1;
                    }

                    for idx in 0..(x + 1) {
                        let t = x - idx;
                        if self.gtable[t][prediction_result.gi[t]].ubit == min {
                            self.gtable[t][prediction_result.gi[t]].tag = self.gtag(pc, t);
                            self.gtable[t][prediction_result.gi[t]].ctr =
                                if taken { 0 } else { -1 };
                            self.gtable[t][prediction_result.gi[t]].ubit = 0;
                            break;
                        }
                    }
                }
            }

            // periodic reset of ubit: reset is not complete but bit by bit
            self.tick += 1;

            if (self.tick & ((1 << 18) - 1)) == 0 {
                let mut mask = (self.tick >> 18) & 1;
                if mask == 0 {
                    mask = 2;
                }
                for idx in 0..NHIST {
                    for idx2 in 0..(1 << LOGG) {
                        self.gtable[idx][idx2].ubit &= mask as i8;
                    }
                }
            }

            // update the counter that provided the prediction, and only this counter

            if prediction_result.bank < NHIST {
                self.gtable[prediction_result.bank][prediction_result.gi[prediction_result.bank]]
                    .ctr = TAGEPredictor::ctrupdate(
                    self.gtable[prediction_result.bank]
                        [prediction_result.gi[prediction_result.bank]]
                        .ctr,
                    taken,
                    CBITS,
                );
            } else {
                // the prediction is from the btable.
                assert!(prediction_result.alternate_prediction == prediction_result.result);

                if prediction_result.result == taken {
                    if taken {
                        if self.btable[prediction_result.bi].pred != 0 {
                            self.btable[prediction_result.bi].hyst = 1;
                        }
                    } else if self.btable[prediction_result.bi].pred == 0 {
                        self.btable[prediction_result.bi].hyst = 0;
                    }
                } else {
                    let mut inter = self.btable[prediction_result.bi].pred * 2
                        + self.btable[prediction_result.bi].hyst;
                    if taken {
                        if inter < 3 {
                            inter += 1;
                        }
                    } else if inter > 0 {
                        inter -= 1;
                    }
                    self.btable[prediction_result.bi].pred = inter >> 1;
                    self.btable[prediction_result.bi].hyst = inter & 1;
                }
            }

            if allocation {
                return BranchPredictorResult::Mispredict;
            } else {
                return BranchPredictorResult::Match;
            }
        }

        // In any case, the history must be updated.
        self.update_history(pc, taken);

        return BranchPredictorResult::NotActive;
    }
}

use serde::ser::SerializeStruct;

use super::BranchPredictorResult;

impl Serialize for TAGEPredictor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("TAGEPredictor", 9)?;
        state.serialize_field("seed", &self.seed)?;
        state.serialize_field("tick", &self.tick)?;
        state.serialize_field("phist", &self.phist)?;
        state.serialize_field("ghist", &self.ghist.as_slice())?;
        state.serialize_field("ch_i", &self.ch_i)?;
        state.serialize_field("ch_t", &self.ch_t)?;
        state.serialize_field("btable", &self.btable.as_slice())?;
        let gtable = self.gtable.iter().map(|x| x.as_slice()).collect::<Vec<_>>();
        state.serialize_field("gtable", &gtable)?;
        state.serialize_field("m", &self.m)?;
        state.end()
    }
}

#[test]
fn test_tage_init() {
    let mut _tage = TAGEPredictor::new();
}
