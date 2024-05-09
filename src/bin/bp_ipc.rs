use worm_cache::components::bp::fetch::FetchUnit;

type FetchUnitType = FetchUnit<{ 1 }>;

fn all_hits() {
    let mut fetch_unit = FetchUnitType::new();
    fetch_unit.train(0, 0, 0, 0);
}

fn main() {}
