pub mod channel;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod remote;

pub use channel::{Error, Event, EventChannel};
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use remote::{IpcError, IpcListener, RemoteEventBus, RemoteSender};
