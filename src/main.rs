// the entrypoint of the functional model of the simulator
// this program takes a trace file as input and outputs the cache state.
// the trace file is encoded in binary and continuous records in the following data structure

use std::env;
use std::io::{BufReader, Write};
use std::{fs::File, io::Read};
use worm_cache::components::memory_ts::{PrivateCacheParameters, TimestampMemoryHierarchy};
use worm_cache::components::NoMMU;

#[repr(C)]
#[cfg(target_pointer_width = "64")]
pub struct TraceEntry {
    paddr: u64,     // 8 bytes
    timestamp: u64, // 8 bytes
    permission: u8, // 1 byte, 0 means instruction, 1 means normal read, 2 means normal write.
    core_id: u8,    // 1 byte
}

impl TraceEntry {
    pub fn read_from_file(file: &mut impl Read) -> Option<TraceEntry> {
        let mut buffer = [0; 18];
        match file.read(&mut buffer) {
            Ok(18) => {
                let block_id = u64::from_le_bytes(buffer[0..8].try_into().unwrap());
                let timestamp = u64::from_le_bytes(buffer[8..16].try_into().unwrap());
                let permission = buffer[16];
                let core_id = buffer[17];
                Some(TraceEntry {
                    paddr: block_id,
                    timestamp,
                    permission,
                    core_id,
                })
            }
            Ok(_) => None,
            Err(_) => None,
        }
    }

    pub fn serialize(&self) -> [u8; 18] {
        let mut buffer = [0; 18];
        buffer[0..8].copy_from_slice(&self.paddr.to_le_bytes());
        buffer[8..16].copy_from_slice(&self.timestamp.to_le_bytes());
        buffer[16] = self.permission;
        buffer[17] = self.core_id;
        buffer
    }
}

fn main() {
    // The parameter of the private cache.
    const P_A: usize = 16;
    const P_S: usize = 2048;

    // The parameter of the shared cache.
    const S_A: usize = 16;
    const S_S: usize = 1024 * 1024;
    println!(
        "Info of the simulator: private cache: {}-way, {} sets; shared cache: {}-way, {} sets",
        P_A, P_S, S_A, S_S
    );

    let args: Vec<_> = env::args().collect();
    if args.len() != 4 {
        println!("Usage: {} <core_count> <trace file> <output file>", args[0]);
        return;
    }

    let core_count: usize = args[1].parse().unwrap();

    let mut mh = TimestampMemoryHierarchy::<NoMMU, { P_A }, { P_S }, { S_A }, { S_S }>::new(core_count);

    // read the trace file.
    let file = File::open(&args[2]).unwrap();
    let mut reader = BufReader::with_capacity(64 * 1024, file);

    loop {
        // now, we read one entry.
        let entry = match TraceEntry::read_from_file(&mut reader) {
            Some(entry) => entry,
            None => break,
        };
        assert!(
            entry.core_id < core_count as u8,
            "core id {} is larger than core count {}",
            entry.core_id,
            core_count
        );
        // simulate that entry.
        let is_instruction = entry.permission == 0;
        let is_write: bool = entry.permission == 2;
        mh.hierarchies(entry.core_id).access_memory(
            entry.timestamp as usize,
            entry.paddr,
            is_instruction,
            is_write,
        );
    }

    // dump the simulation result.
    let mut output_file = File::create(&args[3]).unwrap();
    let mtr = mh.render_mtr::<P_S>();
    let cache_param = PrivateCacheParameters {
        l1i_sets: 64,
        l1i_associativity: 16,
        l1d_sets: 64,
        l1d_associativity: 16,
        l2_sets: P_S,
        l2_associativity: P_A,
        directory_associativity: 0, // this value is not used.
    };

    let cache_hierarchy = mh.render_cache_hierarchy(&mtr, &cache_param);
    let exported_json = serde_json::to_string_pretty(&cache_hierarchy).unwrap();
    output_file.write_all(exported_json.as_bytes()).unwrap();
}
