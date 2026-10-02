mod capture;
mod error;
mod hooks;
mod install;
mod result;
mod save;
mod uninstall;

use std::path::Path;

use crate::run::adapter::{self, Adapter, Installed, Removed};
use crate::run::record::Record;

pub use error::Error;
pub use result::Result;

pub struct Claude;

impl Adapter for Claude {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn capture(&self, stdin: &[u8], now: u64) -> adapter::Result<Option<Record>> {
        Ok(capture::capture(stdin, now)?)
    }

    fn install(&self, home: &Path) -> adapter::Result<Installed> {
        Ok(install::install(home, &hooks::hooks())?)
    }

    fn uninstall(&self, home: &Path) -> adapter::Result<Removed> {
        Ok(uninstall::uninstall(home, &hooks::hooks())?)
    }
}
