//! Một tab query: SQL editor ở trên, kết quả ở dưới.

use std::time::Duration;

use adw::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use quarry_engine::{Event, TabId};
use quarry_model::{ConnId, ConnectionConfig};
use sourceview5::prelude::*;

use crate::app::AppMsg;
use crate::results::ResultGrid;
use crate::sidebar::env_badge;

/// Giới hạn dòng mà engine trả về (khớp `ExecOpts::default()`).
const ROW_LIMIT: u64 = 1000;

#[derive(Debug)]
struct ResultSet {
    grid: Option<ResultGrid>,
    affected: u64,
    truncated: bool,
}

#[derive(Debug)]
pub struct QueryTab {
    pub conn: ConnId,
    pub database: Option<String>,
    /// Widget gốc, dùng làm child của `AdwTabPage`.
    pub root: gtk::Paned,
    buffer: sourceview5::Buffer,
    view: sourceview5::View,
    run_button: gtk::Button,
    stop_button: gtk::Button,
    results: gtk::Stack,
    switcher: gtk::StackSwitcher,
    status: gtk::Label,
    message: gtk::Label,
    sets: Vec<ResultSet>,
    pub running: bool,
}

impl QueryTab {
    pub fn new(
        id: TabId,
        conn: &ConnectionConfig,
        database: Option<String>,
        sql: &str,
        sender: relm4::Sender<AppMsg>,
    ) -> Self {
        // ---------- Editor ----------
        let buffer = sourceview5::Buffer::new(None);
        buffer.set_language(
            sourceview5::LanguageManager::default()
                .language("sql")
                .as_ref(),
        );
        buffer.set_highlight_matching_brackets(true);
        buffer.set_text(sql);
        follow_style_scheme(&buffer);

        let view = sourceview5::View::with_buffer(&buffer);
        view.add_css_class("quarry-editor");
        view.set_monospace(true);
        view.set_show_line_numbers(true);
        view.set_highlight_current_line(true);
        view.set_auto_indent(true);
        view.set_tab_width(4);
        view.set_insert_spaces_instead_of_tabs(true);
        view.set_top_margin(6);
        view.set_bottom_margin(6);
        view.set_left_margin(6);

        // Ctrl+Enter: chạy câu tại con trỏ; Ctrl+Shift+Enter: chạy hết.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let s = sender.clone();
        keys.connect_key_pressed(move |_, key, _, state| {
            let ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
            if ctrl && matches!(key, gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter) {
                let all = state.contains(gtk::gdk::ModifierType::SHIFT_MASK);
                s.emit(AppMsg::RunTab { tab: id, all });
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        view.add_controller(keys);

        let editor_scroll = gtk::ScrolledWindow::new();
        editor_scroll.set_child(Some(&view));
        editor_scroll.set_vexpand(true);

        // ---------- Toolbar ----------
        let run_button = icon_button(
            "media-playback-start-symbolic",
            &gettext("Run Statement (Ctrl+Enter)"),
        );
        let s = sender.clone();
        run_button.connect_clicked(move |_| {
            s.emit(AppMsg::RunTab {
                tab: id,
                all: false,
            })
        });
        let run_all = icon_button(
            "media-seek-forward-symbolic",
            &gettext("Run All (Ctrl+Shift+Enter)"),
        );
        let s = sender.clone();
        run_all.connect_clicked(move |_| s.emit(AppMsg::RunTab { tab: id, all: true }));
        let stop_button = icon_button("media-playback-stop-symbolic", &gettext("Stop"));
        stop_button.set_sensitive(false);
        stop_button.connect_clicked(move |_| sender.emit(AppMsg::CancelTab(id)));

        let context = gtk::Label::new(Some(&context_text(conn, database.as_deref())));
        context.add_css_class("dim-label");
        context.add_css_class("caption");
        context.set_ellipsize(gtk::pango::EllipsizeMode::Middle);

        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        toolbar.add_css_class("toolbar");
        toolbar.add_css_class("quarry-tab-toolbar");
        toolbar.append(&run_button);
        toolbar.append(&run_all);
        toolbar.append(&stop_button);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        toolbar.append(&spacer);
        if let Some(badge) = env_badge(conn.environment) {
            toolbar.append(&badge);
        }
        toolbar.append(&context);

        let editor = gtk::Box::new(gtk::Orientation::Vertical, 0);
        editor.append(&toolbar);
        editor.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        editor.append(&editor_scroll);

        // ---------- Results ----------
        let results = gtk::Stack::new();
        results.set_transition_type(gtk::StackTransitionType::Crossfade);
        results.set_transition_duration(150);
        results.set_vexpand(true);

        let empty = gtk::Label::new(Some(&gettext("Run a query to see results here")));
        empty.add_css_class("dim-label");
        results.add_named(&empty, Some("empty"));

        let spinner = gtk::Spinner::new();
        spinner.set_spinning(true);
        spinner.set_size_request(24, 24);
        spinner.set_halign(gtk::Align::Center);
        spinner.set_valign(gtk::Align::Center);
        results.add_named(&spinner, Some("running"));

        let message = gtk::Label::new(None);
        message.set_wrap(true);
        message.set_selectable(true);
        message.set_xalign(0.0);
        message.set_valign(gtk::Align::Start);
        message.set_margin_top(12);
        message.set_margin_start(12);
        message.set_margin_end(12);
        message.add_css_class("quarry-monospace");
        results.add_named(&message, Some("message"));
        results.set_visible_child_name("empty");

        let switcher = gtk::StackSwitcher::new();
        switcher.set_stack(Some(&results));
        switcher.set_visible(false);

        let status = gtk::Label::new(None);
        status.add_css_class("caption");
        status.add_css_class("dim-label");
        status.set_xalign(0.0);
        status.set_hexpand(true);
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let status_bar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        status_bar.add_css_class("quarry-status-bar");
        status_bar.append(&status);
        status_bar.append(&switcher);

        let bottom = gtk::Box::new(gtk::Orientation::Vertical, 0);
        bottom.append(&results);
        bottom.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        bottom.append(&status_bar);

        let root = gtk::Paned::new(gtk::Orientation::Vertical);
        root.set_start_child(Some(&editor));
        root.set_end_child(Some(&bottom));
        root.set_position(200);
        root.set_shrink_start_child(false);
        root.set_shrink_end_child(false);

        Self {
            conn: conn.id,
            database,
            root,
            buffer,
            view,
            run_button,
            stop_button,
            results,
            switcher,
            status,
            message,
            sets: Vec::new(),
            running: false,
        }
    }

    pub fn focus_editor(&self) {
        self.view.grab_focus();
    }

    /// SQL cần chạy: vùng đang bôi đen, hoặc câu tại con trỏ, hoặc cả buffer (`all`).
    pub fn sql_to_run(&self, all: bool) -> Option<String> {
        if let Some((start, end)) = self.buffer.selection_bounds() {
            return Some(self.buffer.text(&start, &end, false).to_string())
                .filter(|s| !s.trim().is_empty());
        }
        let text = self
            .buffer
            .text(&self.buffer.start_iter(), &self.buffer.end_iter(), false)
            .to_string();
        if all {
            return Some(text).filter(|s| !s.trim().is_empty());
        }
        let cursor_chars = usize::try_from(self.buffer.cursor_position()).unwrap_or(0);
        let cursor = text
            .char_indices()
            .nth(cursor_chars)
            .map_or(text.len(), |(byte, _)| byte);
        let range = quarry_engine::sql::statement_at(&text, cursor)?;
        self.flash_statement(&text, range.clone());
        Some(text[range].to_owned())
    }

    /// Bôi sáng nhẹ câu sắp chạy để người dùng thấy phạm vi.
    fn flash_statement(&self, text: &str, range: std::ops::Range<usize>) {
        let start = text[..range.start].chars().count() as i32;
        let end = text[..range.end].chars().count() as i32;
        let (start, end) = (
            self.buffer.iter_at_offset(start),
            self.buffer.iter_at_offset(end),
        );
        let insert = self.buffer.iter_at_mark(&self.buffer.get_insert());
        self.buffer.select_range(&start, &end);
        let buffer = self.buffer.clone();
        let offset = insert.offset();
        glib::timeout_add_local_once(Duration::from_millis(250), move || {
            let iter = buffer.iter_at_offset(offset);
            buffer.place_cursor(&iter);
        });
    }

    fn set_running(&mut self, running: bool) {
        self.running = running;
        self.run_button.set_sensitive(!running);
        self.stop_button.set_sensitive(running);
    }

    fn clear_results(&mut self) {
        for set in self.sets.drain(..) {
            if let Some(grid) = set.grid {
                self.results.remove(&grid.widget);
            }
        }
        self.switcher.set_visible(false);
    }

    pub fn on_event(&mut self, event: Event) {
        match event {
            Event::QueryStarted { .. } => {
                self.clear_results();
                self.set_running(true);
                self.status.set_text(&gettext("Running…"));
                self.status.remove_css_class("error");
                self.results.set_visible_child_name("running");
            }
            Event::ResultColumns { set, columns, .. } => {
                let grid = ResultGrid::new(&columns);
                let title = format!("{} {}", gettext("Result"), set + 1);
                self.results
                    .add_titled(&grid.widget, Some(&format!("set-{set}")), &title);
                if set == 0 {
                    self.results.set_visible_child(&grid.widget);
                }
                self.ensure_set(set).grid = Some(grid);
            }
            Event::ResultRows { set, rows, .. } => {
                if let Some(grid) = self.ensure_set(set).grid.as_mut() {
                    grid.append(rows);
                }
            }
            Event::ResultDone {
                set,
                affected,
                truncated,
                ..
            } => {
                let entry = self.ensure_set(set);
                entry.affected = affected;
                entry.truncated = truncated;
            }
            Event::QueryFinished { elapsed, .. } => {
                self.set_running(false);
                self.finish(elapsed);
            }
            Event::QueryFailed {
                error, cancelled, ..
            } => {
                self.set_running(false);
                let text = if cancelled {
                    gettext("Query cancelled")
                } else {
                    error
                };
                self.status.set_text(&text);
                self.status.add_css_class("error");
                self.message.set_text(&text);
                if self.sets.iter().all(|s| s.grid.is_none()) {
                    self.results.set_visible_child_name("message");
                }
            }
            _ => {}
        }
    }

    fn ensure_set(&mut self, set: usize) -> &mut ResultSet {
        while self.sets.len() <= set {
            self.sets.push(ResultSet {
                grid: None,
                affected: 0,
                truncated: false,
            });
        }
        &mut self.sets[set]
    }

    fn finish(&mut self, elapsed: Duration) {
        let time = format_duration(elapsed);
        let grids = self.sets.iter().filter(|s| s.grid.is_some()).count();
        self.switcher.set_visible(grids > 1);

        let shown = self.sets.iter().rev().find(|s| s.grid.is_some());
        let summary = match shown {
            Some(set) => {
                let rows = set.grid.as_ref().map_or(0, |g| g.rows);
                let mut text = format!("{} · {time}", plural_rows(rows));
                if set.truncated {
                    text.push_str(&format!(
                        " · {} {ROW_LIMIT} {}",
                        gettext("first"),
                        gettext("rows shown")
                    ));
                }
                text
            }
            None => {
                let affected: u64 = self.sets.iter().map(|s| s.affected).sum();
                let text = format!("{} · {time}", affected_text(affected, self.sets.len()));
                self.message.set_text(&text);
                self.results.set_visible_child_name("message");
                text
            }
        };
        self.status.set_text(&summary);
    }
}

fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon);
    button.add_css_class("flat");
    button.set_tooltip_text(Some(tooltip));
    button
}

fn context_text(conn: &ConnectionConfig, database: Option<&str>) -> String {
    match database {
        Some(db) => format!("{} · {db}", conn.name),
        None => conn.name.clone(),
    }
}

fn plural_rows(rows: u64) -> String {
    if rows == 1 {
        gettext("1 row")
    } else {
        format!("{rows} {}", gettext("rows"))
    }
}

fn affected_text(affected: u64, statements: usize) -> String {
    let statements = if statements == 1 {
        gettext("1 statement")
    } else {
        format!("{statements} {}", gettext("statements"))
    };
    format!("{statements} · {affected} {}", gettext("rows affected"))
}

fn format_duration(d: Duration) -> String {
    if d.as_secs() >= 1 {
        format!("{:.2} s", d.as_secs_f64())
    } else {
        format!("{} ms", d.as_millis())
    }
}

/// Đổi theme editor theo chế độ sáng/tối của hệ thống.
fn follow_style_scheme(buffer: &sourceview5::Buffer) {
    let manager = adw::StyleManager::default();
    let apply = |buffer: &sourceview5::Buffer, dark: bool| {
        let name = if dark { "Adwaita-dark" } else { "Adwaita" };
        buffer.set_style_scheme(
            sourceview5::StyleSchemeManager::default()
                .scheme(name)
                .as_ref(),
        );
    };
    apply(buffer, manager.is_dark());
    let weak = buffer.downgrade();
    manager.connect_dark_notify(move |manager| {
        if let Some(buffer) = weak.upgrade() {
            apply(&buffer, manager.is_dark());
        }
    });
}
