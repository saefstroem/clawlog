#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        dead_code,
        unused_must_use
    )
)]

mod guard;
mod result;
mod run;

use std::{env, io};

use crate::run::{run, Error};
use result::Result;

fn main() {
    let mut stderr = io::stderr();
    // execute guarded run
    guard::guard(
        || {
            let args: Vec<String> = env::args_os()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            run(
                &args,
                &|key| env::var_os(key),
                &mut io::stdin(),
                &mut io::stderr(),
            )
        },
        &mut stderr,
    );
}
