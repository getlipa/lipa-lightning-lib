#[derive(Clone, Debug)]
pub struct TzConfig {
    pub timezone_id: String,
    pub timezone_utc_offset_secs: i32,
}
