use ratatui::widgets::TableState;
use serde_derive::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};

use tui_input::Input;

use std::{collections::BTreeMap, path::PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::{fs::File, io::BufWriter};

use crate::errors::AppError;

const ACTIVITIES_TOML_PATH: &str = "activities.toml";

pub struct Activities {
    current_activity: u8,
    file: ActivitiesFile,
    pub input: Input,
    pub query: Vec<u8>,
    pub table_state: TableState,
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize)]
struct ActivitiesFile {
    /// Used to set current_activity to the last one used before app close if `remember_last` is true.
    last_activity: u8,
    #[serde_as(as = "BTreeMap<DisplayFromStr, _>")]
    activities: BTreeMap<u8, String>,
    /// Items formatted like "Index - Name" to avoid doing it each frame render
    #[serde(skip)]
    formatted: BTreeMap<u8, String>,
}

fn format_activities(activities: &BTreeMap<u8, String>) -> BTreeMap<u8, String> {
    let mut formatted = BTreeMap::new();
    for (index, name) in activities {
        formatted.insert(*index, format!("{index} - {}", name));
    }
    formatted
}

impl Default for ActivitiesFile {
    fn default() -> Self {
        let mut map = BTreeMap::new();
        let no_activity = "N/A".into();
        map.insert(0, no_activity);
        Self {
            last_activity: 0,
            formatted: format_activities(&map),
            activities: map,
        }
    }
}

impl Activities {
    pub fn new() -> Self {
        Self {
            input: Input::default(),
            current_activity: 0,
            file: ActivitiesFile::default(),
            query: Vec::new(),
            table_state: TableState::new(),
        }
    }
    pub async fn save(&mut self) -> Result<(), AppError> {
        self.file.last_activity = self.current_activity;
        let file_path = PathBuf::from(ACTIVITIES_TOML_PATH);
        let file = File::create(&file_path).await?;
        let mut writer = BufWriter::new(file);
        writer
            .write_all(toml::to_string(&self.file)?.as_bytes())
            .await?;
        writer.flush().await?;
        Ok(())
    }
    pub async fn load(&mut self, remember_last: bool) -> Result<(), AppError> {
        let file_path = PathBuf::from(ACTIVITIES_TOML_PATH);
        if !file_path.exists() {
            let file = File::create(&file_path).await?;
            let mut writer = BufWriter::new(file);
            let default = ActivitiesFile::default();
            writer
                .write_all(toml::to_string(&default)?.as_bytes())
                .await?;
            writer.flush().await?;
            self.file = default;
        } else {
            let mut file = File::open(&file_path).await?;
            let mut buffer = String::new();
            file.read_to_string(&mut buffer).await?;
            let new: ActivitiesFile = toml::from_str(&buffer)?;
            self.file = new;
            // TODO bleh
            self.file.formatted = format_activities(&self.file.activities);
            if remember_last && self.file.activities.contains_key(&self.file.last_activity) {
                self.current_activity = self.file.last_activity;
            }
        }
        self.query_from_input();
        if self.selected().is_none() && !self.file.activities.is_empty() {
            self.current_activity = self.query[0];
        }
        Ok(())
    }
    pub fn selected(&self) -> Option<&String> {
        self.file.formatted.get(&self.current_activity)
    }
    pub fn query_from_input(&mut self) {
        let pattern = self.input.to_string().to_lowercase();
        let filtered: Vec<u8> = self
            .file
            .activities
            .iter()
            .filter_map(|(key, value)| {
                let value = format!("{key}{}", value.to_lowercase());
                if value.contains(&pattern) {
                    Some(*key)
                } else {
                    None
                }
            })
            .collect();

        // BTreeMaps's iters "produce their items in order by key"
        self.query = filtered;

        if self.table_state.selected().is_none() && !self.query.is_empty() {
            self.table_state.select(Some(0));
        }
    }
    pub fn select_from_table(&mut self) -> u8 {
        if let Some(new_index) = self.table_state.selected() {
            self.current_activity = self.query[new_index];
        }
        self.current_activity
    }
}

pub mod tui {
    use std::collections::BTreeMap;

    use ratatui::{
        layout::{Alignment, Constraint, Layout},
        style::{Modifier, Style, Stylize},
        widgets::{Block, Borders, Clear, Paragraph, Row, Table},
        Frame,
    };
    use ratatui_macros::{row, text};

    use crate::{app::App, utils::centered_rect};

    pub fn render_activity_selection(app: &mut App, f: &mut Frame) {
        let area = centered_rect(20, 70, f.area());

        // Create the outer block with borders and title
        let block = Block::default()
            .borders(Borders::ALL)
            // .title_style(Style::new().bold())
            // .border_type(BorderType::Thick)
            .border_style(Style::new().yellow())
            .title("Select Activity")
            .title_alignment(Alignment::Center);

        // Draw the block
        f.render_widget(Clear, area);
        f.render_widget(&block, area);

        let vertical = Layout::vertical([
            Constraint::Length(2),
            Constraint::Fill(3),
            Constraint::Length(1),
        ]);
        let inner_area = block.inner(area);
        let [current_area, table_area, input_area] = vertical.areas(inner_area);

        let activity = app.activities.selected();
        let activity: &str = activity.map(|s| s.as_str()).unwrap_or("???");
        let header = Paragraph::new(text!["Current:", activity]).centered();

        f.render_widget(header, current_area);

        let options_block = Block::default()
            .borders(Borders::TOP | Borders::BOTTOM)
            .title_bottom("Search:")
            .title_alignment(Alignment::Center);

        // Activities table here
        f.render_stateful_widget(
            activities_table(&app.activities.file.formatted, &app.activities.query)
                .block(options_block),
            table_area,
            &mut app.activities.table_state,
        );

        let width = input_area.width.max(1) - 1; // So the cursor doesn't bleed off the edge
        let scroll = app.activities.input.visual_scroll(width as usize);
        let input = Paragraph::new(app.activities.input.value()).scroll((0, scroll as u16));
        f.render_widget(input, input_area);
        f.set_cursor_position((
            // Put cursor past the end of the input text
            input_area.x + ((app.activities.input.visual_cursor()).max(scroll) - scroll) as u16,
            input_area.y,
        ));

        // f.render_widget(block, inner_area);
    }

    fn activities_table<'a>(map: &'a BTreeMap<u8, String>, keys: &[u8]) -> Table<'a> {
        let selected_style = Style::default().add_modifier(Modifier::REVERSED);

        let rows: Vec<Row> = keys
            .iter()
            .filter_map(|key| map.get(key).map(|value| row![&**value]))
            .collect();

        let activities_table = Table::new(rows, [Constraint::Percentage(100)])
            .highlight_style(selected_style)
            .highlight_symbol(">> ");

        activities_table
    }
}
