//! Cửa sổ chính: sidebar (cây connection) + vùng nội dung (các tab query).
//!
//! Relm4 chỉ dùng cho component gốc; các phần còn lại (sidebar, dialog, tab) là
//! struct gtk-rs thường, gửi message về đây qua `relm4::Sender<AppMsg>`.

use std::collections::HashMap;

use adw::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use quarry_engine::{Command, ConnState, EngineEvents, EngineHandle, Event, NoticeKind, TabId};
use quarry_model::{ConnId, ObjectKind, TableRef};
use relm4::prelude::*;

use crate::config::{APP_ID, APP_NAME, ISSUE_URL, VERSION, WEBSITE};
use crate::connection_dialog::ConnectionDialog;
use crate::query_tab::QueryTab;
use crate::sidebar::{ConnStatus, DbNode, ObjectsState, Sidebar, Target};

/// Số dòng khi mở một bảng.
const BROWSE_LIMIT: u64 = 1000;

#[derive(Debug)]
pub struct App {
    engine: EngineHandle,
    toasts: adw::ToastOverlay,
    sidebar: Sidebar,
    sidebar_stack: gtk::Stack,
    content_stack: gtk::Stack,
    tab_view: adw::TabView,
    tabs: HashMap<TabId, (QueryTab, adw::TabPage)>,
    next_tab: TabId,
    query_counter: u32,
    dialog: Option<ConnectionDialog>,
    next_request: u64,
    /// Connection được chọn gần nhất (dùng cho Ctrl+T).
    active: Option<(ConnId, Option<String>)>,
}

#[derive(Debug)]
pub enum AppMsg {
    Engine(Event),
    NewConnection,
    EditConnection(ConnId),
    DeleteConnection(ConnId),
    ConfirmDelete(ConnId),
    Connect(ConnId),
    Disconnect(ConnId),
    DialogSave,
    DialogTest,
    DialogClosed,
    TreeActivated(i32),
    TreeContext(i32),
    NewQuery(Option<(ConnId, Option<String>)>),
    RunTab {
        tab: TabId,
        all: bool,
    },
    RunConfirmed {
        tab: TabId,
        conn: ConnId,
        database: Option<String>,
        sql: String,
    },
    CancelTab(TabId),
    TabClosed(gtk::Widget),
    CloseCurrentTab,
    ShowAbout,
}

#[relm4::component(pub)]
impl Component for App {
    type Init = (EngineHandle, EngineEvents);
    type Input = AppMsg;
    type Output = ();
    type CommandOutput = ();

    view! {
        #[root]
        adw::ApplicationWindow {
            set_title: Some(APP_NAME),
            set_default_width: 1280,
            set_default_height: 800,
            set_width_request: 360,
            set_height_request: 294,
            set_content: Some(&toasts),
        }
    }

    fn init(
        (engine, events): Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let input = sender.input_sender().clone();

        // ---------- Sidebar ----------
        let sidebar = Sidebar::new(input.clone());
        let tree_scroll = gtk::ScrolledWindow::new();
        tree_scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        tree_scroll.set_child(Some(&sidebar.list));

        let sidebar_stack = gtk::Stack::new();
        sidebar_stack.add_named(&empty_sidebar(&input), Some("empty"));
        sidebar_stack.add_named(&tree_scroll, Some("tree"));

        let engine_label = gtk::Label::new(Some(&gettext("Starting engine…")));
        engine_label.add_css_class("quarry-sidebar-footer");
        engine_label.add_css_class("caption");
        engine_label.add_css_class("dim-label");
        engine_label.set_xalign(0.0);

        let add_button = gtk::Button::from_icon_name("list-add-symbolic");
        add_button.set_tooltip_text(Some(&gettext("New Connection (Ctrl+N)")));
        let s = input.clone();
        add_button.connect_clicked(move |_| s.emit(AppMsg::NewConnection));
        let sidebar_header = adw::HeaderBar::new();
        sidebar_header.set_title_widget(Some(&adw::WindowTitle::new(APP_NAME, "")));
        sidebar_header.pack_end(&add_button);

        let sidebar_view = adw::ToolbarView::new();
        sidebar_view.add_top_bar(&sidebar_header);
        sidebar_view.set_content(Some(&sidebar_stack));
        sidebar_view.add_bottom_bar(&engine_label);

        // ---------- Content ----------
        let tab_view = adw::TabView::new();
        let s = input.clone();
        tab_view.connect_close_page(move |view, page| {
            s.emit(AppMsg::TabClosed(page.child()));
            view.close_page_finish(page, true);
            glib::Propagation::Stop
        });
        let tab_bar = adw::TabBar::new();
        tab_bar.set_view(Some(&tab_view));
        tab_bar.set_autohide(false);

        let welcome = adw::StatusPage::new();
        welcome.set_icon_name(Some(APP_ID));
        welcome.set_title(&gettext("Welcome to Quarry"));
        welcome.set_description(Some(&gettext(
            "Pick a table in the sidebar, or press Ctrl+T for a new query",
        )));

        let content_stack = gtk::Stack::new();
        content_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        content_stack.set_transition_duration(200);
        content_stack.add_named(&welcome, Some("welcome"));
        let tabs_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        tabs_box.append(&tab_bar);
        tabs_box.append(&tab_view);
        tab_view.set_vexpand(true);
        content_stack.add_named(&tabs_box, Some("tabs"));

        let new_tab = gtk::Button::from_icon_name("tab-new-symbolic");
        new_tab.set_tooltip_text(Some(&gettext("New Query (Ctrl+T)")));
        let s = input.clone();
        new_tab.connect_clicked(move |_| s.emit(AppMsg::NewQuery(None)));
        let menu = gtk::MenuButton::new();
        menu.set_icon_name("open-menu-symbolic");
        menu.set_tooltip_text(Some(&gettext("Main Menu")));
        menu.set_primary(true);
        menu.set_menu_model(Some(&primary_menu()));
        let content_header = adw::HeaderBar::new();
        content_header.set_show_title(false);
        content_header.pack_start(&new_tab);
        content_header.pack_end(&menu);

        let content_view = adw::ToolbarView::new();
        content_view.add_top_bar(&content_header);
        content_view.set_content(Some(&content_stack));

        let split = adw::OverlaySplitView::new();
        split.set_min_sidebar_width(240.0);
        split.set_max_sidebar_width(320.0);
        split.set_sidebar(Some(&sidebar_view));
        split.set_content(Some(&content_view));

        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&split));

        let model = App {
            engine,
            toasts: toasts.clone(),
            sidebar,
            sidebar_stack,
            content_stack,
            tab_view,
            tabs: HashMap::new(),
            next_tab: 1,
            query_counter: 0,
            dialog: None,
            next_request: 1,
            active: None,
        };
        let widgets = view_output!();

        register_actions(&root, &input);

        // Chuyển sự kiện từ engine (thread tokio) về main loop của GTK.
        let s = input.clone();
        glib::spawn_future_local(async move {
            while let Ok(event) = events.recv().await {
                if let Event::Pong { version, .. } = &event {
                    engine_label.set_text(&format!("{} {version}", gettext("Engine ready ·")));
                }
                s.emit(AppMsg::Engine(event));
            }
        });

        model.send(Command::Ping { id: 1 });
        model.send(Command::LoadConnections);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        let input = sender.input_sender().clone();
        match message {
            AppMsg::Engine(Event::ConfirmRequired {
                tab,
                conn,
                database,
                sql,
                message,
            }) => {
                self.confirm_run(tab, conn, database, sql, &message, &input, root);
            }
            AppMsg::Engine(event) => self.on_engine_event(event),
            AppMsg::RunConfirmed {
                tab,
                conn,
                database,
                sql,
            } => {
                self.send(Command::Run {
                    tab,
                    conn,
                    database,
                    sql,
                    confirmed: true,
                });
            }

            AppMsg::NewConnection => self.open_dialog(None, &input, root),
            AppMsg::EditConnection(id) => self.open_dialog(Some(id), &input, root),
            AppMsg::DialogSave => {
                let Some(dialog) = &self.dialog else { return };
                if let Some((config, password, remember_password)) = dialog.read() {
                    self.send(Command::SaveConnection {
                        config,
                        password,
                        remember_password,
                    });
                    dialog.close();
                }
            }
            AppMsg::DialogTest => {
                let Some(dialog) = &self.dialog else { return };
                if let Some((config, password, _)) = dialog.read() {
                    dialog.set_testing();
                    self.next_request += 1;
                    self.send(Command::TestConnection {
                        request: self.next_request,
                        config,
                        password,
                    });
                }
            }
            AppMsg::DialogClosed => self.dialog = None,

            AppMsg::DeleteConnection(id) => self.confirm_delete(id, &input, root),
            AppMsg::ConfirmDelete(id) => self.send(Command::DeleteConnection { id }),
            AppMsg::Connect(id) => self.send(Command::Connect { id }),
            AppMsg::Disconnect(id) => self.send(Command::Disconnect { id }),

            AppMsg::TreeActivated(index) => self.on_tree_activated(index, &input),
            AppMsg::TreeContext(index) => self.show_context_menu(index, &input),

            AppMsg::NewQuery(target) => {
                let target = target
                    .or_else(|| self.active.clone())
                    .or_else(|| self.first_connected());
                match target {
                    Some((conn, database)) => {
                        self.query_counter += 1;
                        let title = format!("{} {}", gettext("Query"), self.query_counter);
                        self.open_tab(conn, database, &title, "", false, &input);
                    }
                    None => self.toast(&gettext("Connect to a database first")),
                }
            }
            AppMsg::RunTab { tab, all } => self.run_tab(tab, all),
            AppMsg::CancelTab(tab) => self.send(Command::Cancel { tab }),
            AppMsg::TabClosed(widget) => {
                let id = self
                    .tabs
                    .iter()
                    .find(|(_, (t, _))| t.root.upcast_ref::<gtk::Widget>() == &widget);
                if let Some(id) = id.map(|(id, _)| *id) {
                    self.tabs.remove(&id);
                    self.send(Command::CloseTab { tab: id });
                }
                self.update_content_page();
            }
            AppMsg::CloseCurrentTab => {
                if let Some(page) = self.tab_view.selected_page() {
                    self.tab_view.close_page(&page);
                }
            }
            AppMsg::ShowAbout => show_about(root),
        }
    }
}

impl App {
    fn send(&self, command: Command) {
        if let Err(err) = self.engine.send(command) {
            tracing::error!(%err, "engine command");
        }
    }

    fn toast(&self, text: &str) {
        self.toasts.add_toast(adw::Toast::new(text));
    }

    fn first_connected(&self) -> Option<(ConnId, Option<String>)> {
        self.sidebar
            .nodes
            .iter()
            .find(|n| n.status == ConnStatus::Connected)
            .map(|n| (n.config.id, n.config.database.clone()))
    }

    // ---------- Engine events ----------

    fn on_engine_event(&mut self, event: Event) {
        match event {
            Event::Pong { .. } => {}
            Event::Notice { kind, message } => {
                if kind == NoticeKind::Error {
                    tracing::error!("{message}");
                }
                self.toast(&message);
            }
            Event::Connections(list) => {
                self.sidebar.set_connections(list);
                self.sidebar_stack
                    .set_visible_child_name(if self.sidebar.is_empty() {
                        "empty"
                    } else {
                        "tree"
                    });
            }
            Event::TestResult { result, .. } => {
                if let Some(dialog) = &self.dialog {
                    dialog.set_test_result(&result);
                }
            }
            Event::ConnState { id, state } => {
                if let ConnState::Failed(err) = &state {
                    let name = self
                        .sidebar
                        .conn(id)
                        .map(|n| n.config.name.clone())
                        .unwrap_or_default();
                    self.toast(&format!("{name}: {err}"));
                }
                if let Some(node) = self.sidebar.conn_mut(id) {
                    node.status = match state {
                        ConnState::Connecting => ConnStatus::Connecting,
                        ConnState::Connected { .. } => {
                            node.expanded = true;
                            ConnStatus::Connected
                        }
                        ConnState::Disconnected => {
                            node.databases.clear();
                            ConnStatus::Idle
                        }
                        ConnState::Failed(err) => ConnStatus::Failed(err),
                    };
                }
                self.sidebar.rebuild();
            }
            Event::Databases { id, databases } => {
                let mut load = None;
                if let Some(node) = self.sidebar.conn_mut(id) {
                    let default_db = node.config.database.clone();
                    node.databases = databases
                        .into_iter()
                        .map(|name| DbNode {
                            name,
                            expanded: false,
                            objects: ObjectsState::NotLoaded,
                        })
                        .collect();
                    // Mở sẵn database mặc định.
                    if let Some(db) = node
                        .databases
                        .iter_mut()
                        .find(|d| Some(&d.name) == default_db.as_ref())
                    {
                        db.expanded = true;
                        db.objects = ObjectsState::Loading;
                        load = Some(db.name.clone());
                    }
                }
                if let Some(database) = load {
                    self.send(Command::LoadObjects { id, database });
                }
                self.sidebar.rebuild();
            }
            Event::Objects {
                id,
                database,
                result,
            } => {
                if let Some(db) = self
                    .sidebar
                    .conn_mut(id)
                    .and_then(|n| n.databases.iter_mut().find(|d| d.name == database))
                {
                    db.objects = match result {
                        Ok(objects) => ObjectsState::Loaded(objects),
                        Err(err) => ObjectsState::Failed(err),
                    };
                }
                self.sidebar.rebuild();
            }
            Event::ConfirmRequired { .. } => {}
            Event::QueryStarted { tab }
            | Event::ResultColumns { tab, .. }
            | Event::ResultRows { tab, .. }
            | Event::ResultDone { tab, .. }
            | Event::QueryFinished { tab, .. }
            | Event::QueryFailed { tab, .. } => {
                if let Some((query_tab, page)) = self.tabs.get_mut(&tab) {
                    query_tab.on_event(event);
                    page.set_loading(query_tab.running);
                }
            }
        }
    }

    // ---------- Sidebar ----------

    fn on_tree_activated(&mut self, index: i32, input: &relm4::Sender<AppMsg>) {
        match self.sidebar.target(index) {
            Target::Connection(id) => {
                let Some(node) = self.sidebar.conn_mut(id) else {
                    return;
                };
                let default_db = node.config.database.clone();
                match node.status {
                    ConnStatus::Connected => node.expanded = !node.expanded,
                    ConnStatus::Connecting => {}
                    ConnStatus::Idle | ConnStatus::Failed(_) => self.send(Command::Connect { id }),
                }
                self.active = Some((id, default_db));
                self.sidebar.rebuild();
            }
            Target::Database(id, name) => {
                self.active = Some((id, Some(name.clone())));
                let Some(db) = self
                    .sidebar
                    .conn_mut(id)
                    .and_then(|n| n.databases.iter_mut().find(|d| d.name == name))
                else {
                    return;
                };
                db.expanded = !db.expanded;
                if db.expanded
                    && matches!(
                        db.objects,
                        ObjectsState::NotLoaded | ObjectsState::Failed(_)
                    )
                {
                    db.objects = ObjectsState::Loading;
                    self.send(Command::LoadObjects { id, database: name });
                }
                self.sidebar.rebuild();
            }
            Target::Object(id, database, object) => {
                self.active = Some((id, Some(database.clone())));
                let sql = match object.kind {
                    ObjectKind::Table | ObjectKind::View | ObjectKind::MaterializedView => {
                        let table = TableRef {
                            database: None,
                            schema: None,
                            name: object.name.clone(),
                        };
                        format!("{};", select_page_sql(&table))
                    }
                    ObjectKind::Function => {
                        format!("SHOW CREATE FUNCTION {};", quote(&object.name))
                    }
                    _ => format!("SHOW CREATE PROCEDURE {};", quote(&object.name)),
                };
                self.open_tab(id, Some(database), &object.name, &sql, true, input);
            }
            Target::Nothing => {}
        }
    }

    fn show_context_menu(&mut self, index: i32, input: &relm4::Sender<AppMsg>) {
        let Target::Connection(id) = self.sidebar.target(index) else {
            return;
        };
        let Some(row) = self.sidebar.row(index) else {
            return;
        };
        let connected = self
            .sidebar
            .conn(id)
            .is_some_and(|n| n.status == ConnStatus::Connected);
        let database = self
            .sidebar
            .conn(id)
            .and_then(|n| n.config.database.clone());

        let popover = gtk::Popover::new();
        popover.set_has_arrow(false);
        popover.add_css_class("menu");
        let items = gtk::Box::new(gtk::Orientation::Vertical, 0);
        type MakeMsg = Box<dyn Fn() -> AppMsg>;
        let mut entries: Vec<(String, MakeMsg)> = Vec::new();
        if connected {
            entries.push((
                gettext("New Query"),
                Box::new(move || AppMsg::NewQuery(Some((id, database.clone())))),
            ));
            entries.push((
                gettext("Disconnect"),
                Box::new(move || AppMsg::Disconnect(id)),
            ));
        } else {
            entries.push((gettext("Connect"), Box::new(move || AppMsg::Connect(id))));
        }
        entries.push((
            gettext("Edit…"),
            Box::new(move || AppMsg::EditConnection(id)),
        ));
        entries.push((
            gettext("Delete…"),
            Box::new(move || AppMsg::DeleteConnection(id)),
        ));

        for (label, make_msg) in entries {
            let button = gtk::Button::with_label(&label);
            button.add_css_class("flat");
            if let Some(child) = button.child().and_downcast::<gtk::Label>() {
                child.set_xalign(0.0);
            }
            let (s, weak) = (input.clone(), popover.downgrade());
            button.connect_clicked(move |_| {
                if let Some(popover) = weak.upgrade() {
                    popover.popdown();
                }
                s.emit(make_msg());
            });
            items.append(&button);
        }
        popover.set_child(Some(&items));
        popover.set_parent(&row);
        popover.connect_closed(|p| {
            let p = p.clone();
            glib::idle_add_local_once(move || p.unparent());
        });
        popover.popup();
    }

    fn open_dialog(
        &mut self,
        id: Option<ConnId>,
        input: &relm4::Sender<AppMsg>,
        root: &adw::ApplicationWindow,
    ) {
        let existing = id
            .and_then(|id| self.sidebar.conn(id))
            .map(|n| n.config.clone());
        let dialog = ConnectionDialog::new(existing.as_ref(), input.clone());
        dialog.present(root);
        self.dialog = Some(dialog);
    }

    fn confirm_delete(
        &self,
        id: ConnId,
        input: &relm4::Sender<AppMsg>,
        root: &adw::ApplicationWindow,
    ) {
        let name = self
            .sidebar
            .conn(id)
            .map(|n| n.config.name.clone())
            .unwrap_or_default();
        let dialog = adw::AlertDialog::new(
            Some(&gettext("Delete Connection?")),
            Some(&format!(
                "“{name}” {}",
                gettext(
                    "and its saved password will be removed. The database itself is not affected."
                )
            )),
        );
        dialog.add_response("cancel", &gettext("Cancel"));
        dialog.add_response("delete", &gettext("Delete"));
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let s = input.clone();
        dialog.connect_response(Some("delete"), move |_, _| {
            s.emit(AppMsg::ConfirmDelete(id))
        });
        dialog.present(Some(root));
    }

    #[allow(clippy::too_many_arguments)]
    fn confirm_run(
        &self,
        tab: TabId,
        conn: ConnId,
        database: Option<String>,
        sql: String,
        message: &str,
        input: &relm4::Sender<AppMsg>,
        root: &adw::ApplicationWindow,
    ) {
        let (name, env) = self
            .sidebar
            .conn(conn)
            .map(|n| (n.config.name.clone(), n.config.environment))
            .unwrap_or_default();
        let heading = match env {
            quarry_model::Environment::Production => gettext("Run on Production?"),
            _ => gettext("Run This Statement?"),
        };
        let dialog = adw::AlertDialog::new(
            Some(&heading),
            Some(&format!("{message}\n\n{}: {name}", gettext("Connection"))),
        );
        dialog.add_response("cancel", &gettext("Cancel"));
        dialog.add_response("run", &gettext("Run"));
        dialog.set_response_appearance("run", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let s = input.clone();
        dialog.connect_response(Some("run"), move |_, _| {
            s.emit(AppMsg::RunConfirmed {
                tab,
                conn,
                database: database.clone(),
                sql: sql.clone(),
            });
        });
        dialog.present(Some(root));
    }

    // ---------- Tabs ----------

    fn open_tab(
        &mut self,
        conn: ConnId,
        database: Option<String>,
        title: &str,
        sql: &str,
        run_now: bool,
        input: &relm4::Sender<AppMsg>,
    ) {
        let Some(config) = self.sidebar.conn(conn).map(|n| n.config.clone()) else {
            return;
        };
        let id = self.next_tab;
        self.next_tab += 1;

        let tab = QueryTab::new(id, &config, database, sql, input.clone());
        let page = self.tab_view.append(&tab.root);
        page.set_title(title);
        page.set_tooltip(&format!(
            "{} · {}",
            config.name,
            tab.database.as_deref().unwrap_or("")
        ));
        self.tab_view.set_selected_page(&page);
        tab.focus_editor();
        self.tabs.insert(id, (tab, page));
        self.update_content_page();

        if run_now {
            self.run_tab(id, true);
        }
    }

    fn run_tab(&mut self, tab: TabId, all: bool) {
        let Some((query_tab, _)) = self.tabs.get(&tab) else {
            return;
        };
        if query_tab.running {
            return;
        }
        let Some(sql) = query_tab.sql_to_run(all) else {
            self.toast(&gettext("Nothing to run"));
            return;
        };
        self.send(Command::Run {
            tab,
            conn: query_tab.conn,
            database: query_tab.database.clone(),
            sql,
            confirmed: false,
        });
    }

    fn update_content_page(&self) {
        let name = if self.tab_view.n_pages() > 0 {
            "tabs"
        } else {
            "welcome"
        };
        self.content_stack.set_visible_child_name(name);
    }
}

fn select_page_sql(table: &TableRef) -> String {
    format!("SELECT * FROM {} LIMIT {BROWSE_LIMIT}", quote(&table.name))
}

fn quote(ident: &str) -> String {
    format!("`{}`", ident.replace('`', "``"))
}

fn empty_sidebar(input: &relm4::Sender<AppMsg>) -> gtk::Box {
    let empty = gtk::Box::new(gtk::Orientation::Vertical, 6);
    empty.add_css_class("quarry-empty");
    empty.set_valign(gtk::Align::Center);

    let icon = gtk::Image::from_icon_name("network-server-symbolic");
    icon.set_pixel_size(32);
    icon.add_css_class("dim-label");
    let title = gtk::Label::new(Some(&gettext("No Connections")));
    title.add_css_class("heading");
    let hint = gtk::Label::new(Some(&gettext("Add a database connection to get started")));
    hint.add_css_class("caption");
    hint.add_css_class("dim-label");
    hint.set_wrap(true);
    hint.set_justify(gtk::Justification::Center);
    let button = gtk::Button::with_label(&gettext("New Connection"));
    button.add_css_class("pill");
    button.add_css_class("suggested-action");
    button.set_halign(gtk::Align::Center);
    button.set_margin_top(12);
    let s = input.clone();
    button.connect_clicked(move |_| s.emit(AppMsg::NewConnection));

    for widget in [
        icon.upcast_ref::<gtk::Widget>(),
        title.upcast_ref(),
        hint.upcast_ref(),
        button.upcast_ref(),
    ] {
        empty.append(widget);
    }
    empty
}

fn primary_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let section = gio::Menu::new();
    section.append(
        Some(&gettext("New _Connection…")),
        Some("win.new-connection"),
    );
    section.append(Some(&gettext("New _Query")), Some("win.new-query"));
    menu.append_section(None, &section);
    let section = gio::Menu::new();
    section.append(Some(&gettext("_About Quarry")), Some("win.about"));
    section.append(Some(&gettext("_Quit")), Some("app.quit"));
    menu.append_section(None, &section);
    menu
}

fn register_actions(window: &adw::ApplicationWindow, input: &relm4::Sender<AppMsg>) {
    let window_action = |name: &str, msg: fn() -> AppMsg| {
        let action = gio::SimpleAction::new(name, None);
        let s = input.clone();
        action.connect_activate(move |_, _| s.emit(msg()));
        window.add_action(&action);
    };
    window_action("about", || AppMsg::ShowAbout);
    window_action("new-connection", || AppMsg::NewConnection);
    window_action("new-query", || AppMsg::NewQuery(None));
    window_action("close-tab", || AppMsg::CloseCurrentTab);

    let app = relm4::main_application();
    let quit = gio::SimpleAction::new("quit", None);
    let weak_app = app.downgrade();
    quit.connect_activate(move |_, _| {
        if let Some(app) = weak_app.upgrade() {
            app.quit();
        }
    });
    app.add_action(&quit);
    app.set_accels_for_action("app.quit", &["<Control>q"]);
    app.set_accels_for_action("win.new-connection", &["<Control>n"]);
    app.set_accels_for_action("win.new-query", &["<Control>t"]);
    app.set_accels_for_action("win.close-tab", &["<Control>w"]);
}

fn show_about(parent: &adw::ApplicationWindow) {
    let dialog = adw::AboutDialog::builder()
        .application_name(APP_NAME)
        .application_icon(APP_ID)
        .version(VERSION)
        .developer_name(gettext("Quarry contributors"))
        .comments(gettext("A fast, native database client for Linux"))
        .license_type(gtk::License::Agpl30)
        .website(WEBSITE)
        .issue_url(ISSUE_URL)
        .build();
    dialog.present(Some(parent));
}
