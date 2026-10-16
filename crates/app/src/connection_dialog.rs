//! Dialog tạo / sửa connection (MySQL / MariaDB).

use adw::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use quarry_engine::Password;
use quarry_model::{ConnId, ConnectionConfig, DriverId, Environment, SslMode};

use crate::app::AppMsg;

const ENVIRONMENTS: [Environment; 3] = [
    Environment::Local,
    Environment::Staging,
    Environment::Production,
];
const SSL_MODES: [SslMode; 5] = [
    SslMode::Disable,
    SslMode::Prefer,
    SslMode::Require,
    SslMode::VerifyCa,
    SslMode::VerifyFull,
];
const DEFAULT_PORT: f64 = 3306.0;

#[derive(Debug)]
pub struct ConnectionDialog {
    pub dialog: adw::Dialog,
    /// `Some` khi sửa connection có sẵn.
    editing: Option<ConnId>,
    name: adw::EntryRow,
    environment: adw::ComboRow,
    host: adw::EntryRow,
    port: adw::SpinRow,
    user: adw::EntryRow,
    password: adw::PasswordEntryRow,
    remember: adw::SwitchRow,
    database: adw::EntryRow,
    ssl: adw::ComboRow,
    read_only: adw::SwitchRow,
    test_row: adw::ActionRow,
    test_spinner: gtk::Spinner,
    test_button: gtk::Button,
}

impl ConnectionDialog {
    pub fn new(existing: Option<&ConnectionConfig>, sender: relm4::Sender<AppMsg>) -> Self {
        let title = if existing.is_some() {
            gettext("Edit Connection")
        } else {
            gettext("New Connection")
        };

        // ---------- Rows ----------
        let name = entry(&gettext("Name"));
        let environment = combo(
            &gettext("Environment"),
            &[
                &gettext("Local"),
                &gettext("Staging"),
                &gettext("Production"),
            ],
        );
        environment.set_subtitle(&gettext(
            "Staging and production ask before risky statements",
        ));

        let host = entry(&gettext("Host"));
        let port = adw::SpinRow::with_range(1.0, 65535.0, 1.0);
        port.set_title(&gettext("Port"));
        port.set_value(DEFAULT_PORT);
        let user = entry(&gettext("User"));
        let password = adw::PasswordEntryRow::new();
        password.set_title(&gettext("Password"));
        let remember = adw::SwitchRow::new();
        remember.set_title(&gettext("Save Password"));
        remember.set_subtitle(&gettext("Stored in the system keyring"));
        remember.set_active(true);
        let database = entry(&gettext("Default Database"));

        let ssl = combo(
            &gettext("SSL"),
            &[
                &gettext("Disabled"),
                &gettext("Preferred"),
                &gettext("Required"),
                &gettext("Verify CA"),
                &gettext("Verify Identity"),
            ],
        );
        ssl.set_selected(1);
        let read_only = adw::SwitchRow::new();
        read_only.set_title(&gettext("Read-Only"));
        read_only.set_subtitle(&gettext("The database rejects every write"));

        let test_spinner = gtk::Spinner::new();
        let test_button = gtk::Button::with_label(&gettext("Test"));
        test_button.set_valign(gtk::Align::Center);
        let test_row = adw::ActionRow::new();
        test_row.set_title(&gettext("Test Connection"));
        test_row.add_suffix(&test_spinner);
        test_row.add_suffix(&test_button);

        // ---------- Layout ----------
        let general = adw::PreferencesGroup::new();
        general.add(&name);
        general.add(&environment);

        let server = adw::PreferencesGroup::new();
        server.set_title(&gettext("Server"));
        for row in [
            host.upcast_ref::<gtk::Widget>(),
            port.upcast_ref(),
            user.upcast_ref(),
            password.upcast_ref(),
        ] {
            server.add(row);
        }
        server.add(&remember);
        server.add(&database);

        let advanced = adw::PreferencesGroup::new();
        advanced.set_title(&gettext("Advanced"));
        advanced.add(&ssl);
        advanced.add(&read_only);

        let test = adw::PreferencesGroup::new();
        test.add(&test_row);

        let page = adw::PreferencesPage::new();
        for group in [&general, &server, &advanced, &test] {
            page.add(group);
        }

        let cancel = gtk::Button::with_label(&gettext("Cancel"));
        let save = gtk::Button::with_label(&gettext("Save"));
        save.add_css_class("suggested-action");
        let header = adw::HeaderBar::new();
        header.set_show_start_title_buttons(false);
        header.set_show_end_title_buttons(false);
        header.pack_start(&cancel);
        header.pack_end(&save);

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&page));

        let dialog = adw::Dialog::new();
        dialog.set_title(&title);
        dialog.set_content_width(480);
        dialog.set_content_height(640);
        dialog.set_child(Some(&toolbar));
        dialog.set_default_widget(Some(&save));

        // ---------- Signals ----------
        let weak = dialog.downgrade();
        cancel.connect_clicked(move |_| {
            if let Some(d) = weak.upgrade() {
                d.close();
            }
        });
        let s = sender.clone();
        save.connect_clicked(move |_| s.emit(AppMsg::DialogSave));
        let s = sender.clone();
        test_button.connect_clicked(move |_| s.emit(AppMsg::DialogTest));
        dialog.connect_closed(move |_| sender.emit(AppMsg::DialogClosed));
        // Bỏ đánh dấu lỗi khi người dùng sửa.
        for row in [&name, &host] {
            row.connect_changed(|row| row.remove_css_class("error"));
        }

        let this = Self {
            dialog,
            editing: existing.map(|c| c.id),
            name,
            environment,
            host,
            port,
            user,
            password,
            remember,
            database,
            ssl,
            read_only,
            test_row,
            test_spinner,
            test_button,
        };
        if let Some(config) = existing {
            this.fill(config);
        } else {
            this.host.set_text("127.0.0.1");
            this.user.set_text("root");
        }
        this
    }

    fn fill(&self, c: &ConnectionConfig) {
        self.name.set_text(&c.name);
        self.environment
            .set_selected(index_of(&ENVIRONMENTS, c.environment));
        self.host.set_text(c.host.as_deref().unwrap_or_default());
        self.port.set_value(c.port.map_or(DEFAULT_PORT, f64::from));
        self.user.set_text(c.user.as_deref().unwrap_or_default());
        self.database
            .set_text(c.database.as_deref().unwrap_or_default());
        self.ssl.set_selected(index_of(&SSL_MODES, c.ssl));
        self.read_only.set_active(c.read_only);
        // Không hiện password đã lưu; để trống = giữ nguyên.
        self.password.set_text("");
        self.password
            .set_tooltip_text(Some(&gettext("Leave empty to keep the saved password")));
    }

    pub fn present(&self, parent: &impl IsA<gtk::Widget>) {
        self.dialog.present(Some(parent));
        self.name.grab_focus();
    }

    pub fn close(&self) {
        self.dialog.close();
    }

    /// Đọc form. `None` (và đánh dấu ô lỗi) nếu thiếu thông tin bắt buộc.
    pub fn read(&self) -> Option<(ConnectionConfig, Option<Password>, bool)> {
        let host = self.host.text().trim().to_owned();
        if host.is_empty() {
            self.host.add_css_class("error");
            self.host.grab_focus();
            return None;
        }
        let name = match self.name.text().trim() {
            "" => host.clone(),
            name => name.to_owned(),
        };
        let non_empty = |s: glib::GString| Some(s.trim().to_owned()).filter(|s| !s.is_empty());
        let config = ConnectionConfig {
            id: self.editing.unwrap_or_default(),
            name,
            driver: DriverId("mysql".to_owned()),
            host: Some(host),
            port: Some(self.port.value() as u16),
            database: non_empty(self.database.text()),
            user: non_empty(self.user.text()),
            path: None,
            ssl: SSL_MODES
                .get(self.ssl.selected() as usize)
                .copied()
                .unwrap_or_default(),
            ssh: None,
            environment: ENVIRONMENTS
                .get(self.environment.selected() as usize)
                .copied()
                .unwrap_or_default(),
            read_only: self.read_only.is_active(),
            color: None,
            group: None,
        };
        let password = Some(self.password.text().to_string())
            .filter(|p| !p.is_empty())
            .map(Password);
        Some((config, password, self.remember.is_active()))
    }

    pub fn set_testing(&self) {
        self.test_spinner.start();
        self.test_button.set_sensitive(false);
        self.test_row.set_subtitle("");
        self.test_row.remove_css_class("success");
        self.test_row.remove_css_class("error");
    }

    pub fn set_test_result(&self, result: &Result<String, String>) {
        self.test_spinner.stop();
        self.test_button.set_sensitive(true);
        match result {
            Ok(version) => {
                self.test_row
                    .set_subtitle(&format!("{} {version}", gettext("Connected · server")));
                self.test_row.add_css_class("success");
            }
            Err(err) => {
                self.test_row.set_subtitle(err);
                self.test_row.add_css_class("error");
            }
        }
    }
}

fn entry(title: &str) -> adw::EntryRow {
    let row = adw::EntryRow::new();
    row.set_title(title);
    row
}

fn combo(title: &str, items: &[&str]) -> adw::ComboRow {
    let row = adw::ComboRow::new();
    row.set_title(title);
    row.set_model(Some(&gtk::StringList::new(items)));
    row
}

fn index_of<T: PartialEq>(list: &[T], value: T) -> u32 {
    list.iter().position(|v| *v == value).unwrap_or(0) as u32
}
