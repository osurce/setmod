use crate::{command, config, db};
use hashbrown::HashMap;

#[derive(Default)]
pub struct Handlers {
    handlers: HashMap<String, Box<dyn command::Handler + Send + 'static>>,
}

impl Handlers {
    /// Insert the given handler.
    pub fn insert(
        &mut self,
        command: impl AsRef<str>,
        handler: impl command::Handler + Send + 'static,
    ) {
        self.handlers
            .insert(command.as_ref().to_string(), Box::new(handler));
    }

    /// Lookup the given command mutably.
    pub fn get_mut(
        &mut self,
        command: &str,
    ) -> Option<&mut (dyn command::Handler + Send + 'static)> {
        self.handlers.get_mut(command).map(|command| &mut **command)
    }
}

mod countdown;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum Config {
    #[serde(rename = "countdown")]
    Countdown(countdown::Config),
}

/// Context for a hook.
pub struct HookContext<'a> {
    pub db: &'a db::Database,
    pub handlers: &'a mut Handlers,
}

pub trait Module {
    /// Set up command handlers for this module.
    fn hook(&self, _: HookContext<'_>) -> Result<(), failure::Error> {
        Ok(())
    }
}

impl Config {
    pub fn load(
        &self,
        config: &config::Config,
    ) -> Result<Box<dyn Module + 'static>, failure::Error> {
        Ok(match *self {
            Config::Countdown(ref module) => {
                Box::new(self::countdown::Countdown::load(config, module)?)
            }
        })
    }
}