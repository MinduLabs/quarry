//! Bảng kết quả (tạm thời dùng `GtkColumnView`, chỉ đọc).
//!
//! Đây là giải pháp tạm cho tới khi có data grid tự vẽ (ADR 0003): đủ cho vài nghìn
//! dòng, chưa có sửa dữ liệu hay chọn vùng.

use adw::prelude::*;
use gtk::{gio, glib};
use quarry_model::{ColumnMeta, Value};

/// Số ký tự tối đa hiển thị trong một ô.
const CELL_MAX_CHARS: usize = 200;

type Row = Vec<Value>;

#[derive(Debug)]
pub struct ResultGrid {
    pub widget: gtk::ScrolledWindow,
    store: gio::ListStore,
    pub rows: u64,
}

impl ResultGrid {
    pub fn new(columns: &[ColumnMeta]) -> Self {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::SingleSelection::new(Some(store.clone()));
        let view = gtk::ColumnView::new(Some(selection));
        view.add_css_class("data-table");
        view.add_css_class("quarry-grid");
        view.set_show_column_separators(true);
        view.set_reorderable(false);

        for (index, meta) in columns.iter().enumerate() {
            let column =
                gtk::ColumnViewColumn::new(Some(&meta.name), Some(cell_factory(index, meta)));
            column.set_resizable(true);
            view.append_column(&column);
        }

        let widget = gtk::ScrolledWindow::new();
        widget.set_child(Some(&view));
        widget.set_vexpand(true);
        Self {
            widget,
            store,
            rows: 0,
        }
    }

    pub fn append(&mut self, rows: Vec<Row>) {
        self.rows += rows.len() as u64;
        let objects: Vec<glib::BoxedAnyObject> =
            rows.into_iter().map(glib::BoxedAnyObject::new).collect();
        self.store.splice(self.store.n_items(), 0, &objects);
    }
}

fn is_numeric_type(type_name: &str) -> bool {
    let t = type_name.to_ascii_uppercase();
    ["INT", "DECIMAL", "NUMERIC", "FLOAT", "DOUBLE", "REAL"]
        .iter()
        .any(|k| t.contains(k))
}

fn cell_factory(index: usize, meta: &ColumnMeta) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    let numeric = is_numeric_type(&meta.type_name);

    factory.connect_setup(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let label = gtk::Label::new(None);
        label.set_xalign(if numeric { 1.0 } else { 0.0 });
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(48);
        label.set_single_line_mode(true);
        item.set_child(Some(&label));
    });

    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let (Some(label), Some(object)) = (
            item.child().and_downcast::<gtk::Label>(),
            item.item().and_downcast::<glib::BoxedAnyObject>(),
        ) else {
            return;
        };
        let row = object.borrow::<Row>();
        let value = row.get(index).unwrap_or(&Value::Null);
        label.set_text(&value.display(CELL_MAX_CHARS));
        if value.is_null() {
            label.add_css_class("quarry-null");
        } else {
            label.remove_css_class("quarry-null");
        }
    });
    factory
}
