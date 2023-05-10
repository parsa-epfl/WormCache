use crate::qemu_api;

pub struct QEMUPluginBasicBlock(pub *mut qemu_api::qemu_plugin_tb);

pub struct QEMUPluginBasicBlockIterator {
    tb: *mut qemu_api::qemu_plugin_tb,
    idx: usize,
    max: usize,
}

pub struct QEMUPluginInstruction(pub *mut qemu_api::qemu_plugin_insn);

impl QEMUPluginBasicBlock {
    #[inline(always)]
    pub unsafe fn instruction_count(&self) -> usize {
        return qemu_api::qemu_plugin_tb_n_insns(self.0);
    }

    pub fn iter(&self) -> QEMUPluginBasicBlockIterator {
        unsafe {
            return QEMUPluginBasicBlockIterator {
                tb: self.0,
                idx: 0,
                max: self.instruction_count(),
            };
        }
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
            };
        }
    }
}

impl Iterator for QEMUPluginBasicBlockIterator {
    type Item = QEMUPluginInstruction;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.idx == self.max {
            return None;
        }
        self.idx += 1;
        unsafe {
            return Some(QEMUPluginInstruction(qemu_api::qemu_plugin_tb_get_insn(
                self.tb,
                self.idx - 1,
            )));
        }
    }
}

impl QEMUPluginInstruction {
    #[inline(always)]
    pub fn physical_address(&self) -> usize {
        unsafe {
            return qemu_api::qemu_plugin_insn_haddr(self.0) as usize;
        }
    }

    #[inline(always)]
    pub fn size(&self) -> usize {
        unsafe {
            return qemu_api::qemu_plugin_insn_size(self.0);
        }
    }

    #[inline(always)]
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

pub struct QEMUMemoryInfo(pub qemu_api::qemu_plugin_meminfo_t);

impl QEMUMemoryInfo {
    #[inline(always)]
    pub unsafe fn translate(&self, va: u64) -> Option<u64> {
        let handler = qemu_api::qemu_plugin_get_hwaddr(self.0, va);
        if qemu_api::qemu_plugin_hwaddr_is_io(handler) {
            return None;
        }
        return Some(qemu_api::qemu_plugin_hwaddr_phys_addr(handler));
    }

    pub unsafe fn is_store_operation(&self) -> bool {
        return qemu_api::qemu_plugin_mem_is_store(self.0);
    }
}
