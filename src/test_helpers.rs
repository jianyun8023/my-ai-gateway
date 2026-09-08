//! Shared unit-test environment fixtures. Never compiled in production.

pub(crate) const TEST_ADMIN_KEY: &str = "test-admin-key";

pub(crate) static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) struct EnvRestore {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvRestore {
    pub(crate) fn set(name: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(name);
        std::env::set_var(name, value);
        Self { name, previous }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        if let Some(value) = &self.previous {
            std::env::set_var(self.name, value);
        } else {
            std::env::remove_var(self.name);
        }
    }
}
