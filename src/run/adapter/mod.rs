mod claude;
mod error;
mod harness;
mod registry;
mod result;

pub use error::Error;
pub use harness::{Adapter, Installed, Removed};
pub use registry::{find, ADAPTERS};
pub use result::Result;
