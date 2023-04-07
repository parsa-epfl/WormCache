pub mod parallel;
pub mod single;
pub mod directory;
pub mod mtmodel;

pub enum CacheReturnResult {
    Miss,
    Hit,
    MissWithEviction(usize),
    MissWithDirtyEviction(usize),
    MissWithWrongPermission // shared -> modified, exclusive.
}