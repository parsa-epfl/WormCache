// the entrypoint of the functional model of the simulator
// this program takes a trace file as input and outputs the cache state.
// the trace file is encoded in binary and continuous records in the following data structure

use std::{fs::File, io::Read};
use std::env;

// use worm_cache::mh::ts_model::TimestampMemoryHierarchy;

#[repr(C)]
pub struct TraceEntry {
    block_id: u64, // 8 bytes
    timestamp: u64, // 8 bytes
    is_read: bool, // 1 byte
    core_id: u8 // 1 byte
}

impl TraceEntry {
    pub fn read_from_file(file: &mut File) -> Option<TraceEntry> {
        let mut buffer = [0; 18];
        match file.read(&mut buffer) {
            Ok(18) => {
                let block_id = u64::from_le_bytes(buffer[0..8].try_into().unwrap());
                let timestamp = u64::from_le_bytes(buffer[8..16].try_into().unwrap());
                let is_read = buffer[16] == 1;
                let core_id = buffer[17];
                Some(TraceEntry { block_id, timestamp, is_read, core_id })
            },
            Ok(_) => None,
            Err(_) => None
        }
    }

    pub fn serialize(&self) -> [u8; 18] {
        let mut buffer = [0; 18];
        buffer[0..8].copy_from_slice(&self.block_id.to_le_bytes());
        buffer[8..16].copy_from_slice(&self.timestamp.to_le_bytes());
        buffer[16] = if self.is_read { 1 } else { 0 };
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
    println!("Info of the simulator: private cache: {}-way, {} sets; shared cache: {}-way, {} sets", P_A, P_S, S_A, S_S);

    let args: Vec<_> = env::args().collect();
    if args.len() != 4 {
        println!("Usage: {} <core_count> <trace file> <output file>", args[0]);
    }

    let core_count: usize = args[1].parse().unwrap();
    _ = core_count
}