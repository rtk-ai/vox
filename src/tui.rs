//! Interactive TUI for human users to configure vox.
//!
//! Launched via `vox setup`. Provides a menu to select backend, voice, language,
//! style, and test speech in real-time. AI agents use CLI flags instead.

use std::io::{self, Stdout};

use anyhow::{Context, Result};
use crossterm::ExecutableCommand;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::backend::{self, SpeakOptions};
use crate::config;
use crate::db;

/// All screens in the TUI.
#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Backend,
    Voice,
    Language,
    Style,
    Volume,
    Test,
}

const VOLUME_PRESETS: &[&str] = &["0.5", "0.75", "1.0", "1.25", "1.5", "2.0", "3.0"];

struct App {
    screen: Screen,
    backends: Vec<&'static str>,
    backend_idx: usize,
    voices: Vec<String>,
    voice_idx: usize,
    languages: Vec<&'static str>,
    lang_idx: usize,
    styles: Vec<&'static str>,
    style_idx: usize,
    volume_idx: usize,
    status: String,
    should_quit: bool,
}

impl App {
    fn new() -> Result<Self> {
        let conn = db::open()?;
        let prefs = db::get_preferences(&conn)?;

        #[cfg(target_os = "macos")]
        let backends = vec![
            "say          \u{2605}\u{2605}\u{2605} quality  \u{26a1} 3s",
            "piper        \u{2605}\u{2605}  quality  \u{26a1} <1s  [Rust]",
            "qwen-native  \u{2605}\u{2605}\u{2605}\u{2605} quality  \u{26a1} 12s  [Rust+Metal]",
            "voxtream     \u{2605}\u{2605}\u{2605}\u{2605}\u{2605} quality  \u{26a1} 170ms [CUDA]",
            "qwen         \u{2605}\u{2605}\u{2605}\u{2605} quality  \u{26a1} 2s   [Python+MLX]",
        ];
        #[cfg(not(target_os = "macos"))]
        let backends = vec![
            "piper        \u{2605}\u{2605}  quality  \u{26a1} <1s  [Rust]",
            "qwen-native  \u{2605}\u{2605}\u{2605}\u{2605} quality  \u{26a1} 3s   [Rust+CUDA]",
            "voxtream     \u{2605}\u{2605}\u{2605}\u{2605}\u{2605} quality  \u{26a1} 170ms [CUDA]",
        ];

        let current_backend = prefs.backend.as_deref().unwrap_or(config::DEFAULT_BACKEND);
        let backend_idx = backends
            .iter()
            .position(|b| b.split_whitespace().next() == Some(current_backend))
            .unwrap_or(0);

        let languages: Vec<&str> = config::SUPPORTED_LANGS.to_vec();
        let lang_idx = prefs
            .lang
            .as_deref()
            .and_then(|l| languages.iter().position(|x| *x == l))
            .unwrap_or(0);

        let styles = vec![
            "(default)",
            "calm",
            "energetic",
            "warm",
            "authoritative",
            "cheerful",
            "serious",
        ];
        let style_idx = prefs
            .style
            .as_deref()
            .and_then(|s| styles.iter().position(|x| *x == s))
            .unwrap_or(0);

        let voices = Self::load_voices(backends[backend_idx]);

        let voice_idx = prefs
            .voice
            .as_deref()
            .and_then(|v| voices.iter().position(|x| x == v))
            .unwrap_or(0);

        let volume_idx = VOLUME_PRESETS.iter().position(|x| *x == "1.0").unwrap_or(2);

        Ok(Self {
            screen: Screen::Backend,
            backends,
            backend_idx,
            voices,
            voice_idx,
            languages,
            lang_idx,
            styles,
            style_idx,
            volume_idx,
            status: "Arrow keys to navigate, Enter to select, Tab to switch section, T to test, S to save, Q to quit".into(),
            should_quit: false,
        })
    }

    fn load_voices(backend_name: &str) -> Vec<String> {
        backend::get_backend(backend_name)
            .and_then(|b| b.list_voices())
            .unwrap_or_else(|_| vec!["(default)".into()])
    }

    fn selected_backend(&self) -> &str {
        // Extract backend name (first word before spaces)
        self.backends[self.backend_idx]
            .split_whitespace()
            .next()
            .unwrap_or("say")
    }

    fn selected_lang(&self) -> &str {
        self.languages[self.lang_idx]
    }

    fn selected_voice(&self) -> Option<&str> {
        let v = self.voices.get(self.voice_idx).map(|s| s.as_str())?;
        if v.starts_with('(') { None } else { Some(v) }
    }

    fn selected_style(&self) -> Option<&str> {
        let s = self.styles[self.style_idx];
        if s == "(default)" { None } else { Some(s) }
    }

    fn selected_volume(&self) -> f32 {
        VOLUME_PRESETS[self.volume_idx].parse().unwrap_or(1.0)
    }

    fn current_list_len(&self) -> usize {
        match self.screen {
            Screen::Backend => self.backends.len(),
            Screen::Voice => self.voices.len(),
            Screen::Language => self.languages.len(),
            Screen::Style => self.styles.len(),
            Screen::Volume => VOLUME_PRESETS.len(),
            Screen::Test => 2,
        }
    }

    fn current_idx(&self) -> usize {
        match self.screen {
            Screen::Backend => self.backend_idx,
            Screen::Voice => self.voice_idx,
            Screen::Language => self.lang_idx,
            Screen::Style => self.style_idx,
            Screen::Volume => self.volume_idx,
            Screen::Test => 0,
        }
    }

    fn set_idx(&mut self, idx: usize) {
        match self.screen {
            Screen::Backend => {
                self.backend_idx = idx;
                let name = self.backends[idx]
                    .split_whitespace()
                    .next()
                    .unwrap_or("say");
                self.voices = Self::load_voices(name);
                self.voice_idx = 0;
            }
            Screen::Voice => self.voice_idx = idx,
            Screen::Language => self.lang_idx = idx,
            Screen::Style => self.style_idx = idx,
            Screen::Volume => self.volume_idx = idx,
            Screen::Test => {}
        }
    }

    fn move_up(&mut self) {
        let idx = self.current_idx();
        if idx > 0 {
            self.set_idx(idx - 1);
        }
    }

    fn move_down(&mut self) {
        let idx = self.current_idx();
        let max = self.current_list_len();
        if idx + 1 < max {
            self.set_idx(idx + 1);
        }
    }

    fn next_screen(&mut self) {
        self.screen = match self.screen {
            Screen::Backend => Screen::Voice,
            Screen::Voice => Screen::Language,
            Screen::Language => Screen::Style,
            Screen::Style => Screen::Volume,
            Screen::Volume => Screen::Test,
            Screen::Test => Screen::Backend,
        };
    }

    fn prev_screen(&mut self) {
        self.screen = match self.screen {
            Screen::Backend => Screen::Test,
            Screen::Voice => Screen::Backend,
            Screen::Language => Screen::Voice,
            Screen::Style => Screen::Language,
            Screen::Volume => Screen::Style,
            Screen::Test => Screen::Volume,
        };
    }

    fn test_speak(&mut self) {
        self.status = format!("Speaking with {} ...", self.selected_backend());
        let opts = SpeakOptions {
            voice: self.selected_voice().map(String::from),
            lang: Some(self.selected_lang().to_string()),
            style: self.selected_style().map(String::from),
            volume: self.selected_volume(),
            ..Default::default()
        };
        let text = match self.selected_lang() {
            "fr" => "Bonjour, ceci est un test de synthese vocale.",
            "es" => "Hola, esta es una prueba de sintesis de voz.",
            "de" => "Hallo, dies ist ein Test der Sprachsynthese.",
            "ja" => "こんにちは、これは音声合成のテストです。",
            "zh" => "你好，这是语音合成测试。",
            _ => "Hello, this is a voice synthesis test.",
        };
        match backend::get_backend(self.selected_backend()) {
            Ok(b) => match b.speak(text, &opts) {
                Ok(()) => self.status = "Test complete.".into(),
                Err(e) => self.status = format!("Error: {e}"),
            },
            Err(e) => self.status = format!("Backend error: {e}"),
        }
    }

    fn save(&mut self) -> Result<()> {
        let conn = db::open()?;
        db::set_preference(&conn, "backend", self.selected_backend())?;
        db::set_preference(&conn, "lang", self.selected_lang())?;
        if let Some(v) = self.selected_voice() {
            db::set_preference(&conn, "voice", v)?;
        }
        if let Some(s) = self.selected_style() {
            db::set_preference(&conn, "style", s)?;
        }
        self.status = "Preferences saved.".into();
        Ok(())
    }
}

fn render_list<'a>(title: &'a str, items: &[&str], selected: usize, active: bool) -> List<'a> {
    let items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let marker = if i == selected { "> " } else { "  " };
            let style = if i == selected && active {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else if i == selected {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            ListItem::new(format!("{marker}{item}")).style(style)
        })
        .collect();

    let border_style = if active {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    List::new(items).block(
        Block::bordered()
            .title(format!(" {title} "))
            .border_style(border_style),
    )
}

fn draw(frame: &mut Frame, app: &App) {
    let outer = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .split(frame.area());

    // Title
    frame.render_widget(
        Paragraph::new(" vox setup — interactive voice configuration").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        outer[0],
    );

    // Main area: 6 columns
    let cols = Layout::horizontal([
        Constraint::Percentage(18),
        Constraint::Percentage(22),
        Constraint::Percentage(12),
        Constraint::Percentage(16),
        Constraint::Percentage(12),
        Constraint::Percentage(20),
    ])
    .split(outer[1]);

    // Backend list
    let backend_items: Vec<&str> = app.backends.to_vec();
    frame.render_widget(
        render_list(
            "Backend",
            &backend_items,
            app.backend_idx,
            app.screen == Screen::Backend,
        ),
        cols[0],
    );

    // Voice list
    let voice_items: Vec<&str> = app.voices.iter().map(|s| s.as_str()).collect();
    frame.render_widget(
        render_list(
            "Voice",
            &voice_items,
            app.voice_idx,
            app.screen == Screen::Voice,
        ),
        cols[1],
    );

    // Language list
    frame.render_widget(
        render_list(
            "Language",
            &app.languages,
            app.lang_idx,
            app.screen == Screen::Language,
        ),
        cols[2],
    );

    // Style list
    frame.render_widget(
        render_list(
            "Style",
            &app.styles,
            app.style_idx,
            app.screen == Screen::Style,
        ),
        cols[3],
    );

    // Volume list
    let volume_items: Vec<&str> = VOLUME_PRESETS.to_vec();
    frame.render_widget(
        render_list(
            "Volume",
            &volume_items,
            app.volume_idx,
            app.screen == Screen::Volume,
        ),
        cols[4],
    );

    // Summary + actions
    let summary = vec![
        Line::from(vec![
            Span::styled("Backend: ", Style::default().fg(Color::DarkGray)),
            Span::styled(app.selected_backend(), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Voice:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                app.selected_voice().unwrap_or("(default)"),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("Lang:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(app.selected_lang(), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Style:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                app.selected_style().unwrap_or("(default)"),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("Volume:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}x", VOLUME_PRESETS[app.volume_idx]),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "[T] Test  [S] Save  [Q] Quit",
            Style::default().fg(Color::Green),
        )),
    ];

    let active_test = app.screen == Screen::Test;
    let border_style = if active_test {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    frame.render_widget(
        Paragraph::new(summary).block(
            Block::bordered()
                .title(" Config ")
                .border_style(border_style),
        ),
        cols[5],
    );

    // Status bar
    frame.render_widget(
        Paragraph::new(app.status.as_str()).block(
            Block::bordered()
                .title(" Status ")
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        outer[2],
    );
}

pub fn run() -> Result<()> {
    let mut app = App::new()?;

    terminal::enable_raw_mode().context("failed to enable raw mode")?;
    io::stdout()
        .execute(EnterAlternateScreen)
        .context("failed to enter alternate screen")?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;

    let result = run_loop(&mut terminal, &mut app);

    terminal::disable_raw_mode().ok();
    io::stdout().execute(LeaveAlternateScreen).ok();

    result
}

fn run_loop(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| draw(f, app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => {
                    app.should_quit = true;
                }
                KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => app.next_screen(),
                KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => app.prev_screen(),
                KeyCode::Char('t') | KeyCode::Enter if app.screen == Screen::Test => {
                    app.test_speak();
                }
                KeyCode::Char('t') => app.test_speak(),
                KeyCode::Char('s') => {
                    if let Err(e) = app.save() {
                        app.status = format!("Save error: {e}");
                    }
                }
                KeyCode::Enter => app.next_screen(),
                _ => {}
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
