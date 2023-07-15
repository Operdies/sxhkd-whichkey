use super::bindings::*;
use crate::sxhkd::bindings;

pub fn parse_config(file: Option<String>) -> Config {
    bindings::get_config(file)
}
