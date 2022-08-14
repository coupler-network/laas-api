#[derive(Debug, Clone, Copy)]
pub struct Seconds(pub i64);

impl Seconds {
    pub fn one_hour() -> Self {
        Self(3600)
    }
}
