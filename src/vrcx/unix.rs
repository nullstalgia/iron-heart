#[derive(Debug)]
pub struct VrcxStartup;

#[allow(clippy::unused_self, reason = "matching windows impl")]
impl VrcxStartup {
    pub const fn new() -> Self {
        Self
    }

    pub const fn vrcx_installed(&self) -> bool {
        false
    }

    pub const fn shortcut_exists(&self) -> bool {
        false
    }
}
