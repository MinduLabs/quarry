use gettextrs::{LocaleCategory, bind_textdomain_codeset, bindtextdomain, setlocale, textdomain};

use crate::config::{GETTEXT_PACKAGE, LOCALEDIR};

/// Gọi một lần ở đầu `main`, trước khi tạo bất kỳ thread nào.
#[allow(unsafe_code)]
pub fn init() {
    // SAFETY: setlocale không thread-safe; hàm này chạy khi process mới chỉ có main thread
    // (engine và GTK được khởi động sau).
    unsafe {
        setlocale(LocaleCategory::LcAll, "");
    }
    // Thiếu file dịch không phải lỗi nghiêm trọng: app chạy tiếng Anh.
    if let Err(err) = bindtextdomain(GETTEXT_PACKAGE, LOCALEDIR)
        .and_then(|_| bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8"))
        .and_then(|_| textdomain(GETTEXT_PACKAGE))
    {
        tracing::warn!(%err, "gettext init failed");
    }
}
