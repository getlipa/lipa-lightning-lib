#[cfg(target_os = "android")]
use android_logger::Config;
use log::Level;
#[cfg(target_os = "ios")]
use oslog::OsLogger;
use std::sync::Once;

fn init_native_logger(min_level: Level) {
    #[cfg(target_os = "android")]
    android_logger::init_once(Config::default().with_min_level(min_level));

    #[cfg(target_os = "ios")]
    OsLogger::new("com.getlipa.lipalightninglib")
        .level_filter(min_level.to_level_filter())
        .init()
        .unwrap();

    #[cfg(all(not(target_os = "android"), not(target_os = "ios")))]
    {
        let _ = min_level;
        unimplemented!("Only Android and iOS have native loggers implemented");
    }
}

static INIT_LOGGER_ONCE: Once = Once::new();

/// Call the function once before instantiating the library to get logs.
/// Subsequent calls will have no effect.
pub fn init_native_logger_once(min_level: Level) {
    INIT_LOGGER_ONCE.call_once(|| init_native_logger(min_level));
}
