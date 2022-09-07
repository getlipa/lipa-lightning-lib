use lightning::util::logger::{Level, Logger, Record};
use log::{log, log_enabled};

pub(crate) struct LightningLogger;

impl Logger for LightningLogger {
    fn log(&self, record: &Record) {
        let level = map_level(record.level);
        if log_enabled!(level) {
            let message = record.args.to_string();
            log!(level, "[{}] {}", record.module_path, message);
        }
    }
}

fn map_level(level: lightning::util::logger::Level) -> log::Level {
    match level {
        Level::Gossip => log::Level::Trace,
        Level::Trace => log::Level::Trace,
        Level::Debug => log::Level::Debug,
        Level::Info => log::Level::Info,
        Level::Warn => log::Level::Warn,
        Level::Error => log::Level::Error,
    }
}
