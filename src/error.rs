use thiserror::Error;

#[derive(Error, Debug)]
pub enum DriverError {
    #[error("Failed to parse device info: {0}")]
    ParseError(String),

    #[error("Layout not found: {0}")]
    LayoutNotFound(String),

    #[error("Device detection timed out after {0} attempts")]
    DetectionTimeout(u32),
}

pub type Result<T> = std::result::Result<T, DriverError>;
