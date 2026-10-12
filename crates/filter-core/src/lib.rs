pub mod icc;
pub mod models;
pub mod platform;
pub mod session;
pub mod storage;
pub mod tone;

pub type Result<T> = std::result::Result<T, String>;

#[cfg(test)]
mod tests;

pub mod image;
