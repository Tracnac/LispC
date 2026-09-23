use std::collections::HashMap;

use super::Value;

pub mod io;
pub mod str;

pub type ModuleFactory = fn() -> Value;

pub fn registry() -> HashMap<String, ModuleFactory> {
    HashMap::from([
        ("str".to_owned(), str::module as ModuleFactory),
        ("io".to_owned(), io::module as ModuleFactory),
    ])
}
