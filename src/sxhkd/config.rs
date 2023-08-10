use super::types::*;

pub fn load_hotkeys_rhkd(file: Option<&str>) -> anyhow::Result<Hotkeys> {
    super::parse::get_config(file)
}
