use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextInput {
    value: String,
    cursor: usize,
}

impl TextInput {
    pub fn new(value: &str) -> Self {
        Self { value: value.to_string(), cursor: value.chars().count() }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set(&mut self, value: &str) {
        *self = Self::new(value);
    }

    pub fn handle(&mut self, key: KeyEvent) -> bool {
        let len = self.value.chars().count();
        match key.code {
            KeyCode::Char(c) if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT => {
                let at = self.byte_index(self.cursor);
                self.value.insert(at, c);
                self.cursor += 1;
                true
            }
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                let at = self.byte_index(self.cursor);
                self.value.remove(at);
                true
            }
            KeyCode::Delete if self.cursor < len => {
                let at = self.byte_index(self.cursor);
                self.value.remove(at);
                true
            }
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                false
            }
            KeyCode::Right => {
                self.cursor = (self.cursor + 1).min(len);
                false
            }
            KeyCode::Home => {
                self.cursor = 0;
                false
            }
            KeyCode::End => {
                self.cursor = len;
                false
            }
            _ => false,
        }
    }

    pub fn window(&self, width: usize) -> (String, usize) {
        if width == 0 {
            return (String::new(), 0);
        }
        let offset = self.cursor.saturating_sub(width - 1);
        let visible = self.value.chars().skip(offset).take(width).collect();
        (visible, self.cursor - offset)
    }

    fn byte_index(&self, char_index: usize) -> usize {
        self.value.char_indices().nth(char_index).map_or(self.value.len(), |(i, _)| i)
    }
}
