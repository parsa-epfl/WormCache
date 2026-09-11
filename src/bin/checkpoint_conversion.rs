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

use serde_json;

use worm_cache::checkpoint::FlexusParameter;
use worm_cache::checkpoint::process_cache_hierarchy;
use worm_cache::checkpoint::process_frontend;
use worm_cache::checkpoint::process_mmus;
use worm_cache::checkpoint::process_sms;

fn main() {
    let args = std::env::args().collect::<Vec<String>>();

    // usage: <pf_checkpoint_folder> <flexus_configuration> <output_folder>
    if args.len() != 4 && args.len() != 5 {
        println!(
            "Usage: {} <pf_checkpoint_folder> <flexus_configuration> <output_folder> <resizing=true>",
            args[0]
        );
        std::process::exit(1);
    }

    // check the folder of the checkpoint.
    // If there are files with name like "(.*)-harvard.json.zstd", it is a harvard cache.

    let check_point_folder = &args[1];
    let flexus_file = std::fs::File::open(&args[2]).unwrap();
    let mut flexus: FlexusParameter = serde_json::from_reader(flexus_file).unwrap();
    let output_folder = &args[3];
    let enable_resizing = if args.len() == 5 {
        args[4].parse::<bool>().unwrap()
    } else {
        true
    };

    flexus.no_resizing = !enable_resizing;

    process_cache_hierarchy(check_point_folder, &flexus, output_folder);
    process_frontend(check_point_folder, &flexus, output_folder);
    process_mmus(check_point_folder, &flexus, output_folder);
    process_sms(check_point_folder, &flexus, output_folder);
}
