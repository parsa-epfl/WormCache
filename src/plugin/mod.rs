use crate::qemu_api;
use std::ffi;

pub mod single_core_cache;
pub mod first_touch;

// API wrapper

struct QEMUPluginBasicBlock(*mut qemu_api::qemu_plugin_tb);

struct QEMUPluginBasicBlockIterator {
    tb: *mut qemu_api::qemu_plugin_tb,
    idx: usize,
    max: usize
}

struct QEMUPluginInstruction(*mut qemu_api::qemu_plugin_insn);

impl QEMUPluginBasicBlock {
    #[inline(always)]
    unsafe fn instruction_count(&self) -> usize {
        return qemu_api::qemu_plugin_tb_n_insns(self.0);
    }
}

impl IntoIterator for QEMUPluginBasicBlock {
    type Item = QEMUPluginInstruction;
    type IntoIter = QEMUPluginBasicBlockIterator;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        unsafe {
            return QEMUPluginBasicBlockIterator {
                tb: self.0,
                idx: 0,
                max: qemu_api::qemu_plugin_tb_n_insns(self.0),
            }
        }   
    }
    
}

impl Iterator for QEMUPluginBasicBlockIterator {
    type Item = QEMUPluginInstruction;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.idx == self.max {
            return None
        }
        self.idx += 1;
        unsafe {
            return Some(QEMUPluginInstruction(qemu_api::qemu_plugin_tb_get_insn(self.tb, self.idx - 1)));
        }
    }
}

impl QEMUPluginInstruction {
    pub fn physical_address(&self) -> usize {
        unsafe {
            return qemu_api::qemu_plugin_insn_haddr(self.0) as usize;
        }
    }

    pub fn size(&self) -> usize {
        unsafe {
            return qemu_api::qemu_plugin_insn_size(self.0);
        }
    }

    pub fn literal(&self) -> Vec<u8> {
        let mut res: Vec<u8> = Vec::with_capacity(self.size());
        unsafe {
            let ptr = qemu_api::qemu_plugin_insn_data(self.0) as *const u8;
            let ptr = std::slice::from_raw_parts(ptr, self.size());

            for x in ptr {
                res.push(*x);
            }
        }
        return res;
    }
}