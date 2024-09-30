use serde_derive::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::{
    fs::{create_dir, File},
    io::BufWriter,
};

use crate::errors::AppError;

const ACTIVITIES_TOML_PATH: &str = "activities.toml";

#[serde_as]
#[derive(Debug, Serialize, Deserialize)]
pub struct Activities {
    #[serde(skip)]
    current_activity: u8,
    /// Used to set current_activity to the last one used before app close if `remember_last` is true.
    last_activity: u8,
    #[serde_as(as = "HashMap<DisplayFromStr, _>")]
    activities: HashMap<u8, String>,
}

impl Activities {
    pub fn new() -> Self {
        Self {
            current_activity: 0,
            activities: HashMap::new(),
            last_activity: 0,
        }
    }
    pub async fn load(&mut self) -> Result<(), AppError> {
        let file_path = PathBuf::from(ACTIVITIES_TOML_PATH);
        if !file_path.exists() {
            let file = File::create(&file_path).await?;
            let mut writer = BufWriter::new(file);
            let default = Self::default();
            writer
                .write_all(toml::to_string(&default)?.as_bytes())
                .await?;
            writer.flush().await?;
            *self = default;
            return Ok(());
        }
        let mut file = File::open(&file_path).await?;
        let mut buffer = String::new();
        file.read_to_string(&mut buffer).await?;
        let new: Activities = toml::from_str(&buffer)?;
        *self = new;
        Ok(())
    }
    pub fn selected(&self) -> Option<(&u8, &String)> {
        self.activities.get_key_value(&self.current_activity)
    }
    pub fn select(&mut self, index: u8) -> Result<(), AppError> {
        todo!()
    }
    //pub fn query(query: &str) -> &[Activity] {}
}

impl Default for Activities {
    fn default() -> Self {
        let mut map = HashMap::new();
        let no_activity = "N/A".into();
        map.insert(0, no_activity);
        Self {
            current_activity: 0,
            activities: map,
            last_activity: 0,
        }
    }
}

mod tui {}
