use super::bindings::*;
use crate::sxhkd::bindings;

pub fn load_hotkeys(file: Option<&str>) -> Hotkeys {
    bindings::get_config(file)
}
