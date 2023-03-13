pub mod parallel;
pub mod single;

pub enum CacheReturnResult {
    Miss,
    Hit,
    MissWithEviction(usize),
    MissWithDirtyEviction(usize),
    MissWithWrongPermission // shared -> modified, exclusive.
}