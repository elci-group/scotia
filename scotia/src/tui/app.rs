//! TUI application state: the harness list, selection, and input focus.

use super::detect::{Harness, detect_harnesses};
use crate::storage::StorageConfig;
use ratatui::widgets::ListState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Focus {
    HarnessList,
    TaskInput,
}

pub(crate) struct App {
    pub(crate) harnesses: Vec<Harness>,
    pub(crate) state: ListState,
    pub(crate) task: String,
    pub(crate) focus: Focus,
    pub(crate) storage: StorageConfig,
    pub(crate) message: Option<String>,
    pub(crate) ran_harness: bool,
}

impl App {
    pub(crate) fn new(storage: StorageConfig) -> Self {
        let harnesses = detect_harnesses();
        let mut state = ListState::default();
        if !harnesses.is_empty() {
            state.select(Some(0));
        }
        Self {
            harnesses,
            state,
            task: String::new(),
            focus: Focus::HarnessList,
            storage,
            message: None,
            ran_harness: false,
        }
    }

    pub(crate) fn selected_harness(&self) -> Option<&Harness> {
        self.state.selected().and_then(|i| self.harnesses.get(i))
    }

    pub(crate) fn next(&mut self) {
        if self.harnesses.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => (i + 1) % self.harnesses.len(),
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub(crate) fn previous(&mut self) {
        if self.harnesses.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.harnesses.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }
}
