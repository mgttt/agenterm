//! Host-neutral errors for native Chassis-L1 loader inspection.

use std::fmt;
use std::io;

#[derive(Debug)]
#[non_exhaustive]
pub enum NativeLoaderError {
    Inspect(io::Error),
    SetExecutable(io::Error),
    NotExecutable,
    InvalidExecutableImage,
}

impl fmt::Display for NativeLoaderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inspect(error) => {
                write!(formatter, "cannot inspect native chassis loader: {error}")
            }
            Self::SetExecutable(error) => {
                write!(
                    formatter,
                    "cannot make native chassis loader executable: {error}"
                )
            }
            Self::NotExecutable => formatter.write_str("native chassis loader is not executable"),
            Self::InvalidExecutableImage => {
                formatter.write_str("native chassis loader is not an executable image")
            }
        }
    }
}

impl std::error::Error for NativeLoaderError {}
