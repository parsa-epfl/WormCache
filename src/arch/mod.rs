pub mod aarch64;

// This is a really dirty hack to use enum as a type parameter...
pub trait ISA {}

#[derive(Debug)]
pub struct AArch64;

impl ISA for AArch64 {}

#[derive(Debug)]
pub struct RV64G;

impl ISA for RV64G {}
