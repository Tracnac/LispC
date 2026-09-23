use std::collections::HashMap;

use super::Value;

pub mod str;

pub type ModuleFactory = fn() -> Value;

pub fn registry() -> HashMap<String, ModuleFactory> {
    HashMap::from([("str".to_owned(), str::module as ModuleFactory)])
}
