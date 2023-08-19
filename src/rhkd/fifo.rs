use crate::rhkd::IpcMessage;
use anyhow::Result;
use std::os::unix::prelude::OpenOptionsExt;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FifoError {
    #[error(transparent)]
    Io(#[from] std::io::Error), // source and Display delegate to anyhow::Error
    #[error("Failed to create FIFO at '{1}': {0}")]
    CreateError(nix::errno::Errno, String),
    #[error("No FIFO configured")]
    FifoNotConfigured,
    #[error("File exists but is not a FIFO")]
    FileExists,
}

pub struct Fifo {
    fifo: std::fs::File,
}
impl Fifo {
    fn open_fifo(path: &str) -> std::io::Result<std::fs::File> {
        std::fs::File::options()
            .write(true)
            .read(true)
            .custom_flags(nix::libc::O_NONBLOCK)
            .open(path)
    }

    fn is_fifo(path: &str) -> Result<bool> {
        use nix::libc::*;
        let stat = nix::sys::stat::stat(path)?.st_mode;
        Ok(stat & S_IFMT == S_IFIFO)
    }

    pub fn new(status_fifo: &str) -> Result<Self, FifoError> {
        let fifo = match Self::is_fifo(status_fifo) {
            Ok(true) => Self::open_fifo(status_fifo)?,
            Ok(false) => return Err(FifoError::FileExists),
            _ => {
                use nix::sys::stat::Mode;
                let file_permissions = 0o644;
                if let Err(e) =
                    nix::unistd::mkfifo(status_fifo, Mode::from_bits_truncate(file_permissions))
                {
                    return Err(FifoError::CreateError(e, status_fifo.to_string()));
                }
                Self::open_fifo(status_fifo)?
            }
        };

        Ok(Fifo { fifo })
    }

    pub fn write_message(&mut self, message: &IpcMessage) -> Result<()> {
        use std::io::prelude::Write;
        let message: Option<String> = match message {
            IpcMessage::BeginChain => "BBegin chain".to_string().into(),
            IpcMessage::EndChain => "EEnd chain".to_string().into(),
            IpcMessage::Timeout => "TTimeout reached".to_string().into(),
            IpcMessage::Hotkey(hk) => format!("H{}", hk).into(),
            IpcMessage::Command(c) => format!("C{}", c).into(),
            // The fifo should only implement the messages supported by SXHKD.
            // Sockets support a wider range of messages and are preferred
            _ => None,
        };
        if let Some(m) = message {
            writeln!(self.fifo, "{}", m)?;
        }
        Ok(())
    }
}
