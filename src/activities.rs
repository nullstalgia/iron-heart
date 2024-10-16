use ratatui::widgets::TableState;
use serde_derive::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use tracing::info;

use tui_input::Input;

use std::{collections::BTreeMap, path::PathBuf};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::app::{App, AppUpdate, SubState};
use crate::broadcast;
use crate::errors::AppError;

const ACTIVITIES_TOML_PATH: &str = "activities.toml";

pub struct Activities {
    pub current_activity: u8,
    file: ActivitiesFile,
    pub input: Input,
    pub query: Vec<u8>,
    pub table_state: TableState,
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize)]
struct ActivitiesFile {
    /// Used to set current_activity to the last one used before app close if `remember_last` is true.
    #[serde(default)]
    last_activity: u8,
    #[serde_as(as = "BTreeMap<DisplayFromStr, _>")]
    activities: BTreeMap<u8, String>,
    /// Items formatted like "Index - Name" to avoid doing it each frame render
    #[serde(skip)]
    formatted: BTreeMap<u8, String>,
}
impl ActivitiesFile {
    fn format(&mut self) {
        self.formatted = formatted_activities(&self.activities);
    }
}
fn formatted_activities(activities: &BTreeMap<u8, String>) -> BTreeMap<u8, String> {
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
            formatted: formatted_activities(&map),
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
        let mut file = File::create(&file_path).await?;
        let buffer = toml::to_string(&self.file)?;
        file.write_all(buffer.as_bytes()).await?;
        file.flush().await?;
        file.sync_all().await?;
        info!("Serialized activities length: {}", buffer.len());
        Ok(())
    }
    pub async fn load(&mut self, remember_last: bool) -> Result<u8, AppError> {
        let file_path = PathBuf::from(ACTIVITIES_TOML_PATH);
        if !file_path.exists() {
            let mut file = File::create(&file_path).await?;
            let default = ActivitiesFile::default();
            file.write_all(toml::to_string(&default)?.as_bytes())
                .await?;
            file.flush().await?;
            file.sync_all().await?;
            self.file = default;
        } else {
            let mut file = File::open(&file_path).await?;
            let mut buffer = String::new();
            file.read_to_string(&mut buffer).await?;
            let new: ActivitiesFile = toml::from_str(&buffer)?;
            self.file = new;
            self.file.format();
            if remember_last && self.file.activities.contains_key(&self.file.last_activity) {
                self.current_activity = self.file.last_activity;
            } else {
                self.current_activity = self.initial_activity();
            }
        }
        self.query_from_input();

        Ok(self.current_activity)
    }
    fn initial_activity(&self) -> u8 {
        if self.file.activities.is_empty() {
            0
        } else {
            *self.file.activities.keys().next().unwrap()
        }
    }
    fn next_activity(&self) -> Option<u8> {
        if self.file.activities.is_empty() {
            Some(0)
        } else if self.file.activities.len() >= u8::MAX as usize {
            None
        } else {
            let mut next = self.initial_activity();
            while self.file.activities.contains_key(&next) {
                next += 1;
            }
            Some(next)
        }
    }
    pub fn create_activity(&mut self) -> Option<u8> {
        let activity = self.next_activity();
        if let Some(new_activity) = activity {
            self.file
                .activities
                .insert(new_activity, self.input.to_string());
            self.file.format();
            self.reset();
            self.current_activity = new_activity;
            // Saved by the new activity's broadcast! later
        }
        activity
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
            // I wonder how fragile this is...
            self.current_activity = self.query[new_index];
        }
        self.current_activity
    }
    pub fn reset(&mut self) {
        self.input.reset();
        self.table_state.select(None);
        self.query_from_input();
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

    use crate::{
        app::{App, SubState},
        utils::centered_rect,
    };

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
            .title_top("Ctrl+N: New")
            .title_bottom("Search:")
            .title_alignment(Alignment::Center);

        // Activities table here
        f.render_stateful_widget(
            activities_table(&app.activities.file.formatted, &app.activities.query)
                .block(options_block),
            table_area,
            &mut app.activities.table_state,
        );

        // To hide new activity name from also appearing in the search bar
        if app.sub_state != SubState::ActivitySelection {
            return;
        };

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

    pub fn render_activity_name_entry(app: &mut App, f: &mut Frame) {
        let mut area = centered_rect(30, 25, f.area());
        area.height = area.height.min(4);
        // let is_renaming = app.sub_state == SubState::ActivityRename;

        // Create the outer block with borders and title
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().green())
            .title("New Activity")
            .title_alignment(Alignment::Center);

        // Draw the block
        f.render_widget(Clear, area);
        f.render_widget(&block, area);

        let vertical = Layout::vertical([
            // Constraint::Length(2),
            Constraint::Max(1),
            Constraint::Fill(1),
        ]);
        let inner_area = block.inner(area);
        let [table_area, input_area] = vertical.areas(inner_area);

        // let activity = app.activities.selected();
        // let activity: &str = activity.map(|s| s.as_str()).unwrap_or("???");
        // let header = Paragraph::new(text!["Current:", activity]).centered();

        // f.render_widget(header, current_area);

        let options_block = Block::default()
            .borders(Borders::TOP | Borders::BOTTOM)
            .title_top("Enter Activity Name:")
            .title_alignment(Alignment::Center);

        f.render_widget(options_block, table_area);

        let width = input_area.width.max(1) - 1; // So the cursor doesn't bleed off the edge
        let scroll = app.activities.input.visual_scroll(width as usize);
        let input = Paragraph::new(app.activities.input.value()).scroll((0, scroll as u16));
        f.render_widget(input, input_area);
        f.set_cursor_position((
            // Put cursor past the end of the input text
            input_area.x + ((app.activities.input.visual_cursor()).max(scroll) - scroll) as u16,
            input_area.y,
        ));
    }
}

impl App {
    pub fn activities_select_prompt(&mut self) {
        if self.settings.activities.enabled {
            self.activities.reset();
            self.sub_state = SubState::ActivitySelection;
        }
    }
    pub fn activities_new_prompt(&mut self) {
        if self.settings.activities.enabled {
            self.activities.reset();
            self.sub_state = SubState::ActivityCreation;
        }
    }
    pub fn activities_enter_pressed(&mut self) {
        match self.sub_state {
            SubState::ActivitySelection => {
                let new_activity = self.activities.select_from_table();
                self.broadcast_activity(new_activity);
                self.sub_state = SubState::None;
            }
            SubState::ActivityCreation => {
                if let Some(new_activity) = self.activities.create_activity() {
                    self.broadcast_activity(new_activity);
                }
                self.sub_state = SubState::None;
            }
            _ => {}
        }
    }
    pub fn broadcast_activity(&mut self, activity: u8) {
        broadcast!(
            self.broadcast_tx,
            AppUpdate::ActivitySelected(activity),
            "Failed to send activity update!"
        );
    }
    pub fn activities_esc_pressed(&mut self) {
        if self.sub_state == SubState::ActivityCreation {
            self.activities.reset();
            self.sub_state = SubState::ActivitySelection;
        } else if self.sub_state == SubState::ActivitySelection {
            self.sub_state = SubState::None;
        }
    }
}
