/// This struct contains the information of a partial BB's size and its physical address.
#[derive(Debug)]
pub struct PBBMetadata {
    pub physical_addr: usize,
    pub instruction_count: u8 // This must be true, because each pBB at most contains 16 instructions.
}

impl PBBMetadata {
    pub fn get_pa(encoding: usize) -> usize {
        // The address is at most 60 bit.
        // The higher 15 bits must be zero on Linux, because the GPA is the HVA, which is in user space.
        return encoding & ((((1 as usize) << 60) - 1) as usize);
    }

    pub fn get_instruction_count(encoding: usize) -> u8 {
        // pBB size - 1 is stored in [63:60]
        return (encoding >> 60) as u8 + 1;
    }

    pub fn from(encoding: usize) -> Self {
        return Self {
            physical_addr: Self::get_pa(encoding),
            instruction_count: Self::get_instruction_count(encoding),
        }
    }

    pub fn encode(&self) -> usize {
        assert!(self.instruction_count <= 16); 
        let shifted_instruction_count = (((self.instruction_count - 1) & 0xf) as usize) << 60;
        let truncated_pa = self.physical_addr & ((1 << 60) - 1);
        assert!(truncated_pa == self.physical_addr, "Assumption of only 60bit PA is wrong!");
        return shifted_instruction_count | truncated_pa;
    }

}