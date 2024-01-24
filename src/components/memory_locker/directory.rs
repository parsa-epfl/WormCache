type CoreId = u8;
type SharerList = Vec<CoreId>;


pub struct DirectoryEntry {
    owner: Option<CoreId>,
    sharers: Vec<CoreId>,
}

pub struct Directory {

}