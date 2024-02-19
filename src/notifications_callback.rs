pub trait NotificationsCallback: Send + Sync {
    fn operation_started(&self, key: String);
    fn operation_successful(&self, key: String);
    fn operation_failed(&self, key: String);
}
