use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::Arc;
use std::time::Duration;

use crate::arena::ArenaManager;
use crate::metrics::MetricsStore;
use crate::simulator::LoadGenerator;
use crate::system::{SystemMonitor, SystemSnapshot};

pub struct App {
    pub active_tab: usize,
    pub selected_endpoint_index: usize,
    pub metrics: Arc<MetricsStore>,
    pub arena: Arc<ArenaManager>,
    pub simulator: Arc<LoadGenerator>,
    pub system_monitor: Arc<SystemMonitor>,
    pub system_snapshot: SystemSnapshot,
    pub should_quit: bool,
}

impl App {
    pub fn new(
        metrics: Arc<MetricsStore>,
        arena: Arc<ArenaManager>,
        simulator: Arc<LoadGenerator>,
        system_monitor: Arc<SystemMonitor>,
    ) -> Self {
        let snapshot = system_monitor.snapshot();
        Self {
            active_tab: 0,
            selected_endpoint_index: 0,
            metrics,
            arena,
            simulator,
            system_monitor,
            system_snapshot: snapshot,
            should_quit: false,
        }
    }

    pub fn next_tab(&mut self) {
        self.active_tab = (self.active_tab + 1) % 4;
    }

    pub fn prev_tab(&mut self) {
        if self.active_tab == 0 {
            self.active_tab = 3;
        } else {
            self.active_tab -= 1;
        }
    }

    pub fn select_tab(&mut self, idx: usize) {
        if idx < 4 {
            self.active_tab = idx;
        }
    }

    pub fn update_snapshot(&mut self) {
        self.system_snapshot = self.system_monitor.snapshot();
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) {
        // Handle Ctrl+C
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Tab => {
                self.next_tab();
            }
            KeyCode::BackTab => {
                self.prev_tab();
            }
            KeyCode::Char('1') => self.select_tab(0),
            KeyCode::Char('2') => self.select_tab(1),
            KeyCode::Char('3') => self.select_tab(2),
            KeyCode::Char('4') => self.select_tab(3),

            KeyCode::Char('s') => {
                self.simulator.toggle();
            }
            KeyCode::Char('a') => {
                self.arena.toggle_companions();
            }
            KeyCode::Char('m') => {
                self.arena.toggle_scenario();
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.simulator.increase_concurrency();
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.simulator.decrease_concurrency();
            }
            KeyCode::Char('b') => {
                let arena_clone = self.arena.clone();
                tokio::spawn(async move {
                    let _ = arena_clone
                        .run_shootout(Duration::from_secs(3), 20, 4)
                        .await;
                });
            }
            KeyCode::Char('v') => {
                let arena_clone = self.arena.clone();
                tokio::spawn(async move {
                    let levels = [8, 16, 32, 64, 128, 256];
                    let _ = arena_clone
                        .run_saturation_matrix(Duration::from_secs(2), &levels, 1)
                        .await;
                });
            }
            KeyCode::Char('r') => {
                self.metrics.reset();
            }
            KeyCode::Up => {
                if self.selected_endpoint_index > 0 {
                    self.selected_endpoint_index -= 1;
                }
            }
            KeyCode::Down => {
                let count = self.metrics.endpoints.read().len();
                if count > 0 && self.selected_endpoint_index + 1 < count {
                    self.selected_endpoint_index += 1;
                }
            }
            _ => {}
        }
    }
}
