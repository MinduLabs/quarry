use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Environment of a connection; drives safe mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Environment {
    #[default]
    Local,
    Staging,
    Production,
}

impl Environment {
    /// Every write statement needs confirmation.
    pub fn confirms_every_write(self) -> bool {
        matches!(self, Environment::Production)
    }

    /// Destructive statements (DROP, TRUNCATE, UPDATE/DELETE without WHERE) need confirmation.
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
