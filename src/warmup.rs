use std::{collections::HashSet};


// Associativity is defined as a constant in order to enable the optimization from the compiler.
pub struct WarmupLatencyCacheLine<const ASSO: usize> {
    data: HashSet<usize>,
}

impl<const N: usize> WarmupLatencyCacheLine<N> {
    pub fn update(&mut self, addr: usize) -> bool {
        if self.data.len() < N {
            self.data.insert(addr);
            if self.data.len() == N {
                return true; // just warm up
            }
        }
        return false;
    }

    pub fn is_full(&self) -> bool {
        return self.data.len() >= N;
    }
}

pub struct WarmupLatencyCache<const ASSO: usize, const SET_COUNT: usize> {
    body: [WarmupLatencyCacheLine<ASSO>; SET_COUNT],
    warmed_count: usize,
    report_counter: usize,
}

impl<const ASSO: usize, const SET_COUNT: usize> WarmupLatencyCache<ASSO, SET_COUNT> {
    pub fn new() -> Self {
        return WarmupLatencyCache {
            body: std::array::from_fn(|i|{
                return WarmupLatencyCacheLine::<ASSO> {
                    data: HashSet::new(),
                }
            }),
            warmed_count: 0,
            report_counter: 0
        }
    }

    pub fn update(&mut self, addr: usize) -> bool {
        let set_id = (addr >> 6) % SET_COUNT;
        let res = self.body[set_id].update(addr);
        if res {
            self.warmed_count += 1;
        }

        self.report_counter += 1;

        if self.report_counter % (1024 * 1024) == 0 {
            // report the warm up count.
            println!("[WormCache]: {}, {}", self.report_counter, self.warmed_count);
        }

        return res;
    }

    pub fn is_warmed(&self) -> bool {
        return self.body.iter().map(WarmupLatencyCacheLine::<ASSO>::is_full).reduce(|x, y| -> bool {
            return x && y;
        }).unwrap();
    }

}

impl<const ASSO: usize, const SET_COUNT: usize> Default for WarmupLatencyCache<ASSO, SET_COUNT> {
    fn default() -> Self {
        Self::new()
    }
}


mod test {
    use super::WarmupLatencyCache;

    #[test]
    fn functionality() {
        let mut cache_body = WarmupLatencyCache::<8, 2>::new();
        for i in 0..(1024 * 1024 + 1) {
            cache_body.update((i % 8) * 64 * 2);
        }
        for i in 0..(1024 * 1024 + 1) {
            cache_body.update((i % 8) * 64 * 2 + 64);
        }
        println!("Warmed:{}", cache_body.is_warmed());
    }
}