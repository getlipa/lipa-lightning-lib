#[allow(unused_imports)]
mod mock;

#[cfg(not(feature = "mocked-sdk"))]
pub use breez_sdk_core::*;

#[cfg(feature = "mocked-sdk")]
pub use mock::*;
