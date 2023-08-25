use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::process::Stdio;

use crate::parser::types::Hotkey;
use anyhow::Result;

pub struct Executor {
    #[allow(unused)]
    // We keep a reference to the file in addition to the filedescriptor so the file gets closed
    // when this is dropped
    file_handle: Option<std::fs::File>,
    redir_fd: Option<RawFd>,
    shell: String,
}

impl Executor {
    pub fn new(redir_file: Option<String>) -> Self {
        let shell = std::env::var("SHELL").unwrap_or("bash".to_string());
        let redir_fd = redir_file
            .map(|r| {
                std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .append(true)
                    .create(true)
                    .open(r)
                    .ok()
            })
            .unwrap_or(None);
        let raw_fd = redir_fd.as_ref().map(|file| file.as_raw_fd());
        Self {
            file_handle: redir_fd,
            redir_fd: raw_fd,
            shell,
        }
    }

    pub fn run(&self, hk: &Hotkey) -> Result<()> {
        let mut cmd = std::process::Command::new(self.shell.as_str());
        let mut cmd = cmd.arg("-c").arg(hk.command.to_string()).stdin(Stdio::null());
        if let Some(fd) = self.redir_fd {
            unsafe {
                cmd = cmd
                    .stdout(Stdio::from_raw_fd(fd))
                    .stderr(Stdio::from_raw_fd(fd))
            }
        }
        let mut cmd = cmd.spawn()?;
        // .spawn()?;
        if hk.sync {
            cmd.wait()?;
        }
        Ok(())
    }
}
