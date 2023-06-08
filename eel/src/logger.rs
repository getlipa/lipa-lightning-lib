use lightning::util::logger::{Level, Logger, Record};
use log::log_enabled;

pub struct LightningLogger;

impl Logger for LightningLogger {
    fn log(&self, record: &Record) {
        let level = map_level(record.level);
        if log_enabled!(target: record.module_path, level) {
            let file = strip_prefix(record.file);
            let location = (record.module_path, "", file, record.line);
            // Using the internal function because log!() allows to provive
            // target to override `module_path`, but not file nor line.
            log::__private_api_log(record.args, level, &location, None);
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

fn strip_prefix(line: &'static str) -> &'static str {
    let index = line.find("lightning").unwrap_or(0);
    &line[index..]
}
