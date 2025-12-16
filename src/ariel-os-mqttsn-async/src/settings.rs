#[derive(Clone, Copy)]
pub struct Settings<'a> {
    client_id: &'a [u8],
    keepalive: u16,
}

impl<'a> Default for Settings<'a> {
    fn default() -> Self {
        Settings {
            client_id: b"ariel",
            keepalive: 60,
        }
    }
}

impl<'a> Settings<'a> {
    pub fn builder() -> SettingsBuilder<'a> {
        SettingsBuilder {
            settings: Settings::default(),
        }
    }

    pub fn client_id(&self) -> &'a [u8] {
        self.client_id
    }

    pub fn keepalive(&self) -> u16 {
        self.keepalive
    }
}

pub struct SettingsBuilder<'a> {
    settings: Settings<'a>,
}

impl<'a> SettingsBuilder<'a> {
    pub fn build(&self) -> Settings<'a> {
        self.settings
    }

    pub fn client_id(&mut self, client_id: &'a str) {
        self.settings.client_id = client_id.as_bytes();
    }

    pub fn keepalive(&mut self, keepalive: u16) {
        self.settings.keepalive = keepalive;
    }
}
