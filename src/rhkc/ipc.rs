use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::{
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{self, Serializer};
use thiserror::Error;

use crate::rhkd::IpcMessage;

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("Serialization error")]
    SerializeError(#[from] serde_json::Error),
    #[error("Failed to send payload.")]
    IoError(#[from] std::io::Error),
}

fn write_to_stream<R>(stream: &mut UnixStream, payload: R) -> Result<(), IpcError>
where
    R: Serialize,
{
    let payload = serde_json::to_vec(&payload)?;
    stream.write_all(&payload)?;
    Ok(())
}

fn send_payload(payload: &[u8]) -> Result<Vec<u8>, IpcError> {
    let mut socket = std::os::unix::net::UnixStream::connect(get_socket_path())?;
    socket.write_all(payload)?;
    socket.shutdown(std::net::Shutdown::Write)?;
    let mut response = vec![];
    socket.read_to_end(&mut response)?;
    Ok(response)
}

#[derive(Debug)]
pub struct IpcRequestObject {
    pub request: IpcRequest,
    client: UnixStream,
}

impl TryFrom<UnixStream> for IpcRequestObject {
    type Error = IpcError;

    fn try_from(mut client: UnixStream) -> Result<Self, Self::Error> {
        let request = client.read_request()?;
        Ok(IpcRequestObject { request, client })
    }
}

impl IpcRequestObject {
    pub fn respond(&mut self, response: IpcResponse) -> Result<(), IpcError> {
        response.send(&mut self.client)?;
        Ok(())
    }

    pub fn send<R>(&mut self, item: R) -> Result<(), IpcError>
    where
        R: Serialize,
    {
        let payload = serde_json::to_vec(&item)?;
        self.client.write_all(&payload)?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum IpcRequest {
    Marco,
    DumpConfig,
    Subscribe,
}
#[derive(Serialize, Deserialize, Debug)]
pub enum IpcResponse {
    Polo,
    ConfigDump(String),
    NotImplemented,
    IpcMessage(IpcMessage),
}

pub trait Ipc: Serialize {
    fn send(&self, stream: &mut UnixStream) -> Result<(), IpcError> {
        write_to_stream(stream, self)?;
        Ok(())
    }
}

impl Ipc for IpcResponse {}

impl IpcRequest {
    pub fn request(&self) -> Result<IpcResponse, IpcError> {
        send_request(self)
    }
}

fn send_request(request: &IpcRequest) -> Result<IpcResponse, IpcError> {
    let payload = serde_json::to_vec(&request)?;
    let response = send_payload(&payload)?;
    Ok(serde_json::from_slice(&response)?)
}

pub fn get_socket_path() -> String {
    std::env::var("RHKD_SOCKET_PATH").unwrap_or(format!(
        "/tmp/rhkd_socket_{}",
        std::env::var("DISPLAY").unwrap_or("_".to_string())
    ))
}

pub struct IpcServer {
    path: PathBuf,
    listener: UnixListener,
}

pub trait IpcClient {
    fn read_request(&mut self) -> Result<IpcRequest, IpcError>;
}

impl IpcClient for UnixStream {
    fn read_request(&mut self) -> Result<IpcRequest, IpcError> {
        let mut de = serde_json::Deserializer::from_reader(self);
        let req = IpcRequest::deserialize(&mut de)?;
        Ok(req)
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        self.unbind()
    }
}

pub fn subscribe() -> Result<(), IpcError> {
    let mut socket = UnixStream::connect(get_socket_path())?;
    let payload = serde_json::to_vec(&IpcRequest::Subscribe)?;
    socket.write_all(&payload)?;

    let stream = serde_json::Deserializer::from_reader(socket).into_iter::<IpcResponse>();

    for item in stream {
        println!("Event: {:?}", item);
    }

    Ok(())
}

pub struct SocketReader {
    connection: UnixStream,
}
impl SocketReader {
    pub fn new() -> Result<Self, IpcError> {
        let connection = UnixStream::connect(get_socket_path())?;
        Ok(Self { connection })
    }
    pub fn try_iter<'a, R>(
        &'a mut self,
    ) -> Result<
        serde_json::StreamDeserializer<'_, serde_json::de::IoRead<&mut UnixStream>, R>,
        IpcError,
    >
    where
        R: Deserialize<'a>,
    {
        let payload = serde_json::to_vec(&IpcRequest::Subscribe)?;
        self.connection.write_all(&payload)?;

        let stream: serde_json::StreamDeserializer<'_, serde_json::de::IoRead<&mut UnixStream>, R> =
            serde_json::Deserializer::from_reader(&mut self.connection).into_iter::<R>();
        Ok(stream)
    }
}

pub struct DroppableListener {
    path: PathBuf,
    pub listener: UnixListener,
}
impl DroppableListener {
    pub fn force() -> Result<Self, IpcError> {
        let path = get_socket_path();
        let _ = std::fs::remove_file(&path);
        Self::new(path.into())
    }
    pub fn new(path: PathBuf) -> Result<Self, IpcError> {
        let listener = UnixListener::bind(&path)?;
        Ok(Self { path, listener })
    }
}

impl Drop for DroppableListener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl IpcServer {
    fn unbind(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
    pub fn force() -> Result<Self, IpcError> {
        let _ = std::fs::remove_file(get_socket_path());
        Self::new()
    }
    pub fn new() -> Result<Self, IpcError> {
        let path = get_socket_path().into();
        let listener = UnixListener::bind(&path)?;
        Ok(Self { path, listener })
    }

    pub fn listener(&mut self) -> &mut UnixListener {
        &mut self.listener
    }

    fn handle_client(mut client: UnixStream) -> Result<(), IpcError> {
        let mut byte_vec = vec![];
        client.read_to_end(&mut byte_vec)?;
        let request: IpcRequest = serde_json::from_slice(&byte_vec)?;
        println!("Received request: {:?}", request);
        IpcResponse::Polo.send(&mut client)?;

        Ok(())
    }

    pub fn listen(&mut self, max: Option<usize>) {
        let mut handled = 0;
        for c in self.listener.incoming() {
            match c {
                Ok(conn) => {
                    std::thread::spawn(|| Self::handle_client(conn));
                }
                Err(err) => {
                    eprintln!("Error establishing connection with client: {}", err);
                }
            }
            handled += 1;
            if let Some(max) = max {
                if handled >= max {
                    return;
                }
            }
        }
    }
}
