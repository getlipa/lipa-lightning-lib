use uniffi_lipalightninglib::NotificationsCallback;

pub struct PrintNotificationsHandler {}

impl NotificationsCallback for PrintNotificationsHandler {
    fn operation_started(&self, key: String) {
        println!("Operation started: {key}");
    }

    fn operation_successful(&self, key: String) {
        println!("Operation completed successfully: {key}");
    }

    fn operation_failed(&self, key: String) {
        println!("Operation failed: {key}");
    }
}
