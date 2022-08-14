#[derive(Debug, Clone)]
pub struct Hex(String);

impl Hex {
    pub fn encode(data: &[u8]) -> Self {
        Hex(hex::encode(data))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
