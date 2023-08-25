use crate::{
    parser::*,
    rhkc::ipc::{BindCommand, UnbindCommand},
};
use anyhow::{bail, Context, Result};
use thiserror::Error;

pub struct Config {
    path: Option<String>,
    hotkeys: Vec<Hotkey>,
}

#[derive(Error, Debug)]
pub enum CycleError {
    #[error("Provided hotkey is not a cycle.")]
    NotACycle,
    #[error("Provided cycle was not found.")]
    CycleNotFound,
}

#[derive(Debug, Error)]
pub enum AddBindingError {
    #[error("Hotkey not added because it would interfere with an existing hotkey. Current: {current}, new: {new}")]
    WouldInterfere { current: String, new: String },
}

pub struct AddBindingsResult {
    pub added: Vec<Hotkey>,
    pub removed: Vec<Hotkey>,
    pub errors: Vec<AddBindingError>,
}

impl Config {
    fn get_first_interfering(new: &Hotkey, set: &[Hotkey]) -> Option<usize> {
        set.iter().position(|hk| {
            hk.chain
                .iter()
                .zip(new.chain.iter())
                .all(|(a, b)| a.eq_relaxed(b))
        })
    }
    fn is_prefix_of(short: &[Chord], long: &[Chord]) -> bool {
        if short.len() > long.len() {
            return false;
        }
        for (s, l) in short.iter().zip(long) {
            if !s.eq_relaxed(l) {
                return false;
            }
        }
        true
    }
    pub fn delete_bindings(&mut self, unbind: &UnbindCommand) -> anyhow::Result<Vec<Hotkey>> {
        let chords = crate::parser::parse_chord_chain(&unbind.hotkey)?;
        let (remove, keep): (Vec<_>, Vec<_>) = self
            .hotkeys
            .clone()
            .into_iter()
            .partition(|e| Self::is_prefix_of(&chords, &e.chain));
        self.hotkeys = keep;
        Ok(remove)
    }

    pub fn add_bindings(&mut self, bind: &BindCommand) -> anyhow::Result<AddBindingsResult> {
        let mut result = AddBindingsResult {
            added: vec![],
            removed: vec![],
            errors: vec![],
        };
        let mut binding_text = String::new();
        if let Some(ref title) = bind.title {
            binding_text.push_str(&format!("# {}\n", title));
        }
        if let Some(ref description) = bind.description {
            binding_text.push_str(&format!("# {}\n", description));
        }
        binding_text.push_str(&format!("{}\n", bind.hotkey));
        binding_text.push_str(&format!("  {}\n", bind.command));

        let new = load_config_from_bytes(binding_text.as_bytes())?;
        let new_hotkeys = new.hotkeys;

        // If overwrite is set, remove all interfering keys
        if bind.overwrite {
            self.hotkeys.retain(|this| {
                let retain = !new_hotkeys.iter().any(|hk| {
                    this.chain
                        .iter()
                        .zip(hk.chain.iter())
                        .all(|(a, b)| a.eq_relaxed(b))
                });
                if !retain {
                    result.removed.push(this.clone());
                }
                retain
            });
        }

        for hk in new_hotkeys.into_iter() {
            if !bind.overwrite {
                let current = &self.hotkeys;
                if let Some(idx) = Self::get_first_interfering(&hk, current) {
                    let current = &current[idx];
                    result.errors.push(AddBindingError::WouldInterfere {
                        current: current.chain_repr(),
                        new: hk.chain_repr(),
                    });
                    continue;
                }
            }
            result.added.push(hk.clone());
            self.hotkeys.push(hk);
        }
        Ok(result)
    }
    pub fn get_hotkeys_mut(&mut self) -> &mut Vec<Hotkey> {
        &mut self.hotkeys
    }

    pub fn into_hotkeys(self) -> Vec<Hotkey> {
        self.hotkeys
    }
    pub fn get_hotkeys(&self) -> &Vec<Hotkey> {
        &self.hotkeys
    }

    pub fn reload(&mut self) -> Result<Config> {
        if self.path.is_none() {
            Ok(Config {
                path: None,
                hotkeys: vec![],
            })
        } else {
            load_config(self.path.as_deref())
        }
    }

    pub fn cycle_hotkey(&mut self, hk: &Hotkey) -> Result<(), CycleError> {
        if hk.cycle.is_none() {
            return Err(CycleError::NotACycle);
        };

        let start = self
            .hotkeys
            .iter()
            .position(|h| h == hk)
            .ok_or(CycleError::CycleNotFound)?;
        let hk = &self.hotkeys[start];
        let cycle_period = hk
            .cycle
            .clone()
            .map(|c| c.period)
            .ok_or(CycleError::NotACycle)? as usize;

        // This should cycle the hotkeys so e.g.:
        // [ 1, 2, 3 ] -> [ 2, 3, 1 ] -> [ 3, 1, 2 ] when this method is called in succession
        for idx in start..start + cycle_period - 1 {
            self.hotkeys.swap(idx, idx + 1);
        }
        Ok(())
    }
}

pub fn load_config(file: Option<&str>) -> Result<Config> {
    let path = file
        .map(|s| s.to_string())
        .or_else(|| guess_config_path().ok());
    let Some(path) = path else {
        println!("No config file found. Using empty default config.");
        return Ok(Config {
            path: None,
            hotkeys: vec![],
        });
    };

    let content = std::fs::read(&path).context(format!("Failed to read file '{}'", path))?;
    load_config_from_bytes(&content)
        .context(format!("Error while parsing config '{}'", path))
        .map(|f| Config {
            path: Some(path),
            hotkeys: f.hotkeys,
        })
}

pub fn load_config_from_bytes(content: &[u8]) -> Result<Config> {
    let tokens = Scanner::scan(content)?;
    let tree = token_parser::Parser::build(content, &tokens)?;
    let (hotkeys, errors) = tree.get_hotkeys();
    for error in errors {
        if let Some(err) = error.downcast_ref::<ConfigParseError>() {
            println!("{}", err.contextualize(content));
        } else {
            println!("WARNING: {}", error);
        }
    }

    Ok(Config {
        path: None,
        hotkeys: hotkeys.to_vec(),
    })
}

fn guess_config_path() -> anyhow::Result<String> {
    let config_home = if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME") {
        std::path::PathBuf::from(config_home)
    } else if let Ok(home) = std::env::var("HOME") {
        std::path::Path::new(&home).join(".config")
    } else {
        bail!("Unable to find config file. Neither HOME or XDG_CONFIG_HOME is set.")
    };
    let candidates = [("rhkd", "rhkdrc"), ("sxhkd", "sxhkdrc")];
    let path = candidates.iter().find_map(move |(dir, filename)| {
        let path = config_home.join(dir).join(filename);
        if path.is_file() {
            Some(path.to_string_lossy().to_string())
        } else {
            None
        }
    });
    path.context("Unable to find config file.")
}
