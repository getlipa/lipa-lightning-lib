use lightning::util::logger::{Logger, Record};
#[cfg(target_os = "android")]
use {lightning::util::logger::Level, log::log};
#[cfg(target_os = "ios")]
use {lightning::util::logger::Level, log::log};
#[cfg(all(not(target_os = "android"), not(target_os = "ios")))]
use {std::fs, std::io::Write, time::OffsetDateTime};

pub(crate) struct LipaLogger {}

impl Logger for LipaLogger {
    fn log(&self, record: &Record) {
        let raw_log = record.args.to_string();
        #[cfg(all(not(target_os = "android"), not(target_os = "ios")))]
        {
            let log = format!(
                "{} {:<5} [{}:{}] {}\n",
                // Note that a "real" lightning node almost certainly does *not* want subsecond
                // precision for message-receipt information as it makes log entries a target for
                // deanonymization attacks. For testing, however, its quite useful.
                //Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                OffsetDateTime::now_utc(),
                record.level,
                record.module_path,
                record.line,
                raw_log
            );
            let logs_file_path = ".ldk/logs.txt".to_string();
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(logs_file_path)
                .unwrap()
                .write_all(log.as_bytes())
                .unwrap();
        }
        #[cfg(target_os = "android")]
        match record.level {
            Level::Gossip => {
                log!(log::Level::Trace, "{}", raw_log);
            }
            Level::Trace => {
                log!(log::Level::Trace, "{}", raw_log);
            }
            Level::Debug => {
                log!(log::Level::Debug, "{}", raw_log);
            }
            Level::Info => {
                log!(log::Level::Info, "{}", raw_log);
            }
            Level::Warn => {
                log!(log::Level::Warn, "{}", raw_log);
            }
            Level::Error => {
                log!(log::Level::Error, "{}", raw_log);
            }
        }
        #[cfg(target_os = "ios")]
        match record.level {
            Level::Gossip => {
                log!(log::Level::Trace, "{}", raw_log);
            }
            Level::Trace => {
                log!(log::Level::Trace, "{}", raw_log);
            }
            Level::Debug => {
                log!(log::Level::Debug, "{}", raw_log);
            }
            Level::Info => {
                log!(log::Level::Info, "{}", raw_log);
            }
            Level::Warn => {
                log!(log::Level::Warn, "{}", raw_log);
            }
            Level::Error => {
                log!(log::Level::Error, "{}", raw_log);
            }
        }
    }
}
