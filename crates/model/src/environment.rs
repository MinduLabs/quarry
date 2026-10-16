use serde::{Deserialize, Serialize};

/// Môi trường của một connection — quyết định mức độ safe mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    #[default]
    Local,
    Staging,
    Production,
}

impl Environment {
    /// Câu lệnh ghi có cần người dùng xác nhận không.
    pub fn confirms_every_write(self) -> bool {
        matches!(self, Environment::Production)
    }

    /// Câu lệnh nguy hiểm (DELETE/UPDATE không WHERE, DROP, TRUNCATE) có cần xác nhận không.
    pub fn confirms_destructive(self) -> bool {
        !matches!(self, Environment::Local)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safety_levels() {
        assert!(!Environment::Local.confirms_destructive());
        assert!(Environment::Staging.confirms_destructive());
        assert!(!Environment::Staging.confirms_every_write());
        assert!(Environment::Production.confirms_every_write());
    }
}
