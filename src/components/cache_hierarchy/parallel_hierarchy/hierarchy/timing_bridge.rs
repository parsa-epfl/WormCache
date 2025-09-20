use spin::Mutex;
use std::sync::OnceLock;

use crate::{components::cache_hierarchy::common::SharerList, parameter::CORE_COUNT};

#[repr(C)]
pub struct Raw {
    seq: u64,  // id
    src: u64,  // which core
    addr: u64, // PA
    list: u64, // sharer list
    flag: u64, // directiro hit, llc hit, and broadcast invalidation/forward
    time: u64, // timestamp
}

struct BufPtr {
    ptr: *mut Raw,
    current_idx: u64,
    max_len: u64,
    seq: u64,
}

unsafe impl Send for BufPtr {}
unsafe impl Sync for BufPtr {}

static BUFFER: OnceLock<Mutex<BufPtr>> = OnceLock::new();

#[unsafe(no_mangle)]
unsafe extern "C" fn dev_trace_init(buf: *mut Raw, max_len: u64) {
    let buf_ptr = BufPtr {
        ptr: buf,
        current_idx: 0,
        max_len: max_len,
        seq: 0,
    };
    BUFFER.get_or_init(|| Mutex::new(buf_ptr));
}

pub fn timing_bridge_push(
    core_id: u32,
    pa: u64,
    sharer_list: SharerList,
    llc_hit: bool,
    is_broadcast_invalidation: bool,
    is_forward: bool,
    ts: u64,
) {
    // cast the buf into an array of Raw
    // then push the data into the array
    assert!(CORE_COUNT <= 32);

    if let Some(mutex) = BUFFER.get() {
        let mut buf_ptr = mutex.lock();
        let dst = unsafe { buf_ptr.ptr.add(buf_ptr.current_idx as usize) };

        let flag = (llc_hit as u64) << 2
                | (is_broadcast_invalidation as u64) << 0
                | (is_forward as u64) << 1;

        unsafe {
            assert_eq!(std::ptr::read(std::ptr::addr_of!((*dst).flag)), 0x8,
                      "timing bridge buffer overflowed!!! addr: {:x} seq: {:x}",
                       dst as u64,
                       std::ptr::read(std::ptr::addr_of!((*dst).seq)));

            std::ptr::write(std::ptr::addr_of_mut!((*dst).seq),  buf_ptr.seq);
            std::ptr::write(std::ptr::addr_of_mut!((*dst).src),  core_id as u64);
            std::ptr::write(std::ptr::addr_of_mut!((*dst).addr), pa);
            std::ptr::write(std::ptr::addr_of_mut!((*dst).list), sharer_list.data[0]);
            std::ptr::write(std::ptr::addr_of_mut!((*dst).time), ts);

            std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

            // update flag after all other fields are visible
            std::ptr::write(std::ptr::addr_of_mut!((*dst).flag), flag);
        }

        buf_ptr.seq += 1;
        buf_ptr.current_idx = buf_ptr.seq % buf_ptr.max_len; // wrap around if exceed max_len
    }
}
