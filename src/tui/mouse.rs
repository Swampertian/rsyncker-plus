use ratatui::layout::{Position, Rect};

use crate::tui::app::Action;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Row(usize),
    Action(Action),
    Field(usize),
    Submit,
    Cancel,
    Discover,
    Confirm,
    Reject,
}

#[derive(Debug, Clone, Default)]
pub struct Hits {
    areas: Vec<(Rect, Target)>,
}

impl Hits {
    pub fn clear(&mut self) {
        self.areas.clear();
    }

    pub fn add(&mut self, area: Rect, target: Target) {
        self.areas.push((area, target));
    }

    pub fn at(&self, column: u16, row: u16) -> Option<Target> {
        let position = Position::new(column, row);
        self.areas.iter().rev().find(|(area, _)| area.contains(position)).map(|(_, target)| *target)
    }

    pub fn area_of(&self, target: Target) -> Option<Rect> {
        self.areas.iter().find(|(_, t)| *t == target).map(|(area, _)| *area)
    }
}
