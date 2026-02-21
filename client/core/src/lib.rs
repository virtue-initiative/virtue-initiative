pub const DEFAULT_BASE_API_URL: &str = "https://api.bepure.app";

pub mod auth;
pub mod error;
pub mod image_pipeline;
pub mod models;
pub mod queue;
pub mod schedule;
pub mod token_store;
pub mod upload;

pub use auth::{AuthClient, AuthClientConfig};
pub use error::{CoreError, CoreResult};
pub use image_pipeline::{ImageOutputFormat, ImagePipeline, ImagePipelineConfig, ProcessedImage};
pub use queue::{BufferedUpload, PersistentQueue, QueueEnqueueResult};
pub use schedule::{CaptureSchedulePolicy, CaptureScheduleState, RetryPolicy};
pub use token_store::{FileTokenStore, MemoryTokenStore, TokenStore};
pub use upload::{QueueProcessReport, UploadClient, UploadClientConfig};
