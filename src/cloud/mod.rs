use anyhow::Result;
use serde_json::Value;

pub mod lc_api;
pub mod lc_socket;
pub mod query;

#[cfg(test)]
mod tests;

pub use query::{CloudVerdict, LcCloud};

pub trait CloudTrait {
    fn notify(&self, data: Value) -> Result<()>;
    fn authorize(&self, data: Value) -> Result<CloudVerdict>;
}
