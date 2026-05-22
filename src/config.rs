use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub field1: String,
    pub field2: i32,
}

impl Config {
    pub fn load_from_yaml(file_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let file = std::fs::File::open(file_path)?;
        let config: Config = serde_yaml::from_reader(file)?;
        Ok(config)
    }
}