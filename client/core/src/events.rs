pub mod bus;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod remote;

use serde::{Deserialize, Serialize};

pub use bus::{Emitter, Error, Event, EventBus, EventChannel, Observer, StateType};
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use remote::{IpcError, IpcListener, RemoteEventBus, RemoteSender};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ping;
