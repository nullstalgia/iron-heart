#[derive(Debug)]
pub struct VrcxStartup;

impl VrcxStartup {
    pub fn new() -> Self {
        Self
    }

    pub fn vrcx_installed(&self) -> bool {
        false
    }

    pub fn shortcut_exists(&self) -> bool {
        false
    }
}
