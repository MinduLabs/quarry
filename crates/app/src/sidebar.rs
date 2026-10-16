//! Cây connection → database → bảng/view ở sidebar.
//!
//! Dữ liệu nằm trong [`Sidebar::nodes`]; mỗi lần thay đổi thì dựng lại các dòng của
//! `GtkListBox` (`.navigation-sidebar`). Đủ nhanh cho vài nghìn dòng; khi cần hơn thì
//! chuyển sang `GtkListView` + `GtkTreeListModel`.

use adw::prelude::*;
use gettextrs::gettext;
use quarry_model::{ConnId, ConnectionConfig, Environment, ObjectKind, SchemaObject};

use crate::app::AppMsg;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnStatus {
    Idle,
    Connecting,
    Connected,
    Failed(String),
}

#[derive(Debug)]
pub enum ObjectsState {
    NotLoaded,
    Loading,
    Loaded(Vec<SchemaObject>),
    Failed(String),
}

#[derive(Debug)]
pub struct DbNode {
    pub name: String,
    pub expanded: bool,
    pub objects: ObjectsState,
}

#[derive(Debug)]
pub struct ConnNode {
    pub config: ConnectionConfig,
    pub status: ConnStatus,
    pub expanded: bool,
    pub databases: Vec<DbNode>,
}

/// Dòng được kích hoạt trỏ tới cái gì.
#[derive(Debug, Clone)]
pub enum Target {
    Connection(ConnId),
    Database(ConnId, String),
    Object(ConnId, String, SchemaObject),
    Nothing,
}

#[derive(Debug)]
pub struct Sidebar {
    pub list: gtk::ListBox,
    pub nodes: Vec<ConnNode>,
    targets: Vec<Target>,
}

impl Sidebar {
    pub fn new(sender: relm4::Sender<AppMsg>) -> Self {
        let list = gtk::ListBox::new();
        list.add_css_class("navigation-sidebar");
        list.set_selection_mode(gtk::SelectionMode::Single);

        let s = sender.clone();
        list.connect_row_activated(move |_, row| s.emit(AppMsg::TreeActivated(row.index())));

        // Chuột phải → menu ngữ cảnh.
        let click = gtk::GestureClick::new();
        click.set_button(gtk::gdk::BUTTON_SECONDARY);
        let weak_list = list.downgrade();
        click.connect_pressed(move |_, _, _, y| {
            if let Some(list) = weak_list.upgrade()
                && let Some(row) = list.row_at_y(y as i32)
            {
                list.select_row(Some(&row));
                sender.emit(AppMsg::TreeContext(row.index()));
            }
        });
        list.add_controller(click);

        Self {
            list,
            nodes: Vec::new(),
            targets: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Cập nhật danh sách connection, giữ trạng thái mở/đã kết nối của connection cũ.
    pub fn set_connections(&mut self, list: Vec<ConnectionConfig>) {
        let mut old: Vec<ConnNode> = std::mem::take(&mut self.nodes);
        self.nodes = list
            .into_iter()
            .map(
                |config| match old.iter().position(|n| n.config.id == config.id) {
                    Some(i) => {
                        let mut node = old.swap_remove(i);
                        node.config = config;
                        node
                    }
                    None => ConnNode {
                        config,
                        status: ConnStatus::Idle,
                        expanded: false,
                        databases: Vec::new(),
                    },
                },
            )
            .collect();
        self.nodes.sort_by_key(|n| n.config.name.to_lowercase());
        self.rebuild();
    }

    pub fn conn_mut(&mut self, id: ConnId) -> Option<&mut ConnNode> {
        self.nodes.iter_mut().find(|n| n.config.id == id)
    }

    pub fn conn(&self, id: ConnId) -> Option<&ConnNode> {
        self.nodes.iter().find(|n| n.config.id == id)
    }

    pub fn target(&self, index: i32) -> Target {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.targets.get(i))
            .cloned()
            .unwrap_or(Target::Nothing)
    }

    pub fn row(&self, index: i32) -> Option<gtk::ListBoxRow> {
        self.list.row_at_index(index)
    }

    /// Dựng lại toàn bộ dòng từ `nodes`, giữ lại dòng đang được chọn.
    pub fn rebuild(&mut self) {
        let selected = self.list.selected_row().map(|r| r.index());
        self.list.remove_all();
        let mut targets = Vec::new();
        let list = &self.list;

        for node in &self.nodes {
            let id = node.config.id;
            let expandable = node.status == ConnStatus::Connected;
            let row = row_box(
                0,
                expandable.then_some(node.expanded),
                "network-server-symbolic",
                &node.config.name,
            );
            if let Some(badge) = env_badge(node.config.environment) {
                row.append(&badge);
            }
            match &node.status {
                ConnStatus::Connecting => row.append(&spinner()),
                ConnStatus::Failed(err) => {
                    let icon = gtk::Image::from_icon_name("dialog-error-symbolic");
                    icon.add_css_class("error");
                    icon.set_tooltip_text(Some(err));
                    row.append(&icon);
                }
                ConnStatus::Connected => {
                    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                    dot.add_css_class("quarry-connected-dot");
                    dot.set_valign(gtk::Align::Center);
                    dot.set_tooltip_text(Some(&gettext("Connected")));
                    row.append(&dot);
                }
                ConnStatus::Idle => {}
            }
            push(
                list,
                &mut targets,
                row,
                Target::Connection(id),
                Some(&node.config.name),
            );

            if !(node.expanded && expandable) {
                continue;
            }
            for db in &node.databases {
                let row = row_box(1, Some(db.expanded), "folder-symbolic", &db.name);
                if matches!(db.objects, ObjectsState::Loading) {
                    row.append(&spinner());
                }
                push(
                    list,
                    &mut targets,
                    row,
                    Target::Database(id, db.name.clone()),
                    None,
                );
                if !db.expanded {
                    continue;
                }
                match &db.objects {
                    ObjectsState::Loaded(objects) if objects.is_empty() => {
                        push(
                            list,
                            &mut targets,
                            placeholder(2, &gettext("No tables")),
                            Target::Nothing,
                            None,
                        );
                    }
                    ObjectsState::Loaded(objects) => {
                        for object in sorted(objects) {
                            let row = row_box(2, None, object_icon(object.kind), &object.name);
                            push(
                                list,
                                &mut targets,
                                row,
                                Target::Object(id, db.name.clone(), object.clone()),
                                None,
                            );
                        }
                    }
                    ObjectsState::Failed(err) => {
                        push(
                            list,
                            &mut targets,
                            placeholder(2, err),
                            Target::Nothing,
                            None,
                        );
                    }
                    ObjectsState::Loading | ObjectsState::NotLoaded => {}
                }
            }
        }

        self.targets = targets;
        if let Some(index) = selected
            && let Some(row) = self.list.row_at_index(index)
        {
            self.list.select_row(Some(&row));
        }
    }
}

fn push(
    list: &gtk::ListBox,
    targets: &mut Vec<Target>,
    content: gtk::Box,
    target: Target,
    tooltip: Option<&str>,
) {
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&content));
    if let Some(tip) = tooltip {
        row.set_tooltip_text(Some(tip));
    }
    if matches!(target, Target::Nothing) {
        row.set_activatable(false);
        row.set_selectable(false);
    }
    list.append(&row);
    targets.push(target);
}

fn sorted(objects: &[SchemaObject]) -> Vec<&SchemaObject> {
    let rank = |k: ObjectKind| match k {
        ObjectKind::Table => 0,
        ObjectKind::View | ObjectKind::MaterializedView => 1,
        _ => 2,
    };
    let mut list: Vec<&SchemaObject> = objects.iter().collect();
    list.sort_by(|a, b| {
        rank(a.kind)
            .cmp(&rank(b.kind))
            .then_with(|| a.name.cmp(&b.name))
    });
    list
}

fn object_icon(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Table => "view-grid-symbolic",
        ObjectKind::View | ObjectKind::MaterializedView => "view-list-symbolic",
        _ => "system-run-symbolic",
    }
}

/// Nội dung một dòng: thụt lề theo cấp, mũi tên mở/đóng (nếu có), icon, tên.
fn row_box(depth: i32, expanded: Option<bool>, icon: &str, label: &str) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.set_margin_start(depth * 14);

    let arrow = gtk::Image::new();
    arrow.set_pixel_size(12);
    arrow.add_css_class("dim-label");
    if let Some(open) = expanded {
        arrow.set_icon_name(Some(if open {
            "pan-down-symbolic"
        } else {
            "pan-end-symbolic"
        }));
    }
    content.append(&arrow);

    content.append(&gtk::Image::from_icon_name(icon));

    let text = gtk::Label::new(Some(label));
    text.set_xalign(0.0);
    text.set_hexpand(true);
    text.set_ellipsize(gtk::pango::EllipsizeMode::End);
    content.append(&text);
    content
}

fn placeholder(depth: i32, text: &str) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.set_margin_start(depth * 14 + 18);
    let label = gtk::Label::new(Some(text));
    label.add_css_class("dim-label");
    label.add_css_class("caption");
    label.set_xalign(0.0);
    label.set_wrap(true);
    content.append(&label);
    content
}

fn spinner() -> gtk::Spinner {
    let spinner = gtk::Spinner::new();
    spinner.start();
    spinner
}

/// Badge môi trường, chỉ hiện cho staging/production (luôn kèm chữ, không chỉ màu).
pub fn env_badge(env: Environment) -> Option<gtk::Label> {
    let (text, class) = match env {
        Environment::Local => return None,
        Environment::Staging => (gettext("STAGING"), "staging"),
        Environment::Production => (gettext("PROD"), "production"),
    };
    let badge = gtk::Label::new(Some(&text));
    badge.add_css_class("quarry-env-badge");
    badge.add_css_class(class);
    badge.set_valign(gtk::Align::Center);
    Some(badge)
}
