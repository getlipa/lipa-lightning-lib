use lightning::util::logger::{Level, Logger, Record};
use log::log_enabled;

pub struct LightningLogger;

impl Logger for LightningLogger {
    fn log(&self, record: &Record) {
        let level = map_level(record.level);
        if log_enabled!(target: record.module_path, level) {
            let file = strip_prefix(record.file);
            let location = (record.module_path, "", file, record.line);

            log::logger().log(
                &log::Record::builder()
                    .args(record.args)
                    .level(level)
                    .target(location.0)
                    .module_path_static(Some(location.1))
                    .file_static(Some(location.2))
                    .line(Some(location.3))
                    .build(),
            );
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
