use serde_json;

use worm_cache::checkpoint::process_cache_hierarchy;
use worm_cache::checkpoint::process_frontend;
use worm_cache::checkpoint::process_mmus;
use worm_cache::checkpoint::FlexusParameter;

fn main() {
    let args = std::env::args().collect::<Vec<String>>();

    // usage: <pf_checkpoint_folder> <flexus_configuration> <output_folder>
    if args.len() != 4 {
        println!(
            "Usage: {} <pf_checkpoint_folder> <flexus_configuration> <output_folder>",
            args[0]
        );
        std::process::exit(1);
    }

    // check the folder of the checkpoint.
    // If there are files with name like "(.*)-harvard.json.zstd", it is a harvard cache.

    let check_point_folder = &args[1];
    let flexus_file = std::fs::File::open(&args[2]).unwrap();
    let flexus: FlexusParameter = serde_json::from_reader(flexus_file).unwrap();
    let output_folder = &args[3];

    process_cache_hierarchy(check_point_folder, &flexus, output_folder);
    process_frontend(check_point_folder, &flexus, output_folder);
    process_mmus(check_point_folder, &flexus, output_folder);
}
