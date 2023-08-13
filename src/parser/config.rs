use crate::parser::*;
use anyhow::{bail, Context, Result};

pub struct Config {
    path: Option<String>,
    hotkeys: Vec<Hotkey>,
}

impl Config {
    pub fn get_hotkeys(&self) -> &Vec<Hotkey> {
        &self.hotkeys
    }

    pub fn reload(&mut self) -> Result<Config> {
        load_config(self.path.as_deref())
    }
}

pub fn load_config(file: Option<&str>) -> Result<Config> {
    let path = file
        .map(|s| s.to_string())
        .or_else(|| guess_config_path().ok());
    let Some(path) = path else {
        println!("No config file found. Using empty default config.");
        return Ok(Config { path: None, hotkeys: vec![] });
    };

    let content = std::fs::read(&path).context(format!("Failed to read file '{}'", path))?;
    let tokens = Scanner::scan(&content).context(format!("Error while parsing '{}'", path))?;
    let tree = token_parser::Parser::build(&content, &tokens)?;
    let (hotkeys, errors) = tree.get_hotkeys();
    for error in errors {
        if let Some(err) = error.downcast_ref::<ConfigParseError>() {
            println!("{}", err.contextualize(&content));
        } else {
            println!("WARNING: {}", error);
        }
    }

    Ok(Config {
        path: Some(path),
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
