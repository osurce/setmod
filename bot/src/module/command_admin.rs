use crate::{auth, command, db, module, prelude::*};
use parking_lot::RwLock;
use std::sync::Arc;

pub struct Handler<'a> {
    pub enabled: Arc<RwLock<bool>>,
    pub commands: &'a db::Commands,
}

impl<'a> command::Handler for Handler<'a> {
    fn scope(&self) -> Option<auth::Scope> {
        Some(auth::Scope::Command)
    }

    fn handle<'m>(&mut self, mut ctx: command::Context<'_, '_>) -> Result<(), failure::Error> {
        if !*self.enabled.read() {
            return Ok(());
        }

        let next = command_base!(ctx, self.commands, "!command", "command");

        match next {
            Some("edit") => {
                ctx.check_moderator()?;

                let name = ctx_try!(ctx.next_str("<name>", "!command edit"));
                let template = ctx_try!(ctx.rest_parse("<name> <template>", "!command edit"));
                self.commands.edit(ctx.user.target, name, template)?;

                ctx.respond("Edited command.");
            }
            None | Some(..) => {
                ctx.respond("Expected: show, list, edit, delete, enable, disable, or group.");
            }
        }

        Ok(())
    }
}

pub struct Module;

impl super::Module for Module {
    fn ty(&self) -> &'static str {
        "command"
    }

    fn hook(
        &self,
        module::HookContext {
            handlers,
            futures,
            commands,
            settings,
            ..
        }: module::HookContext<'_, '_>,
    ) -> Result<(), failure::Error> {
        let mut vars = settings.vars();
        let enabled = vars.var("command/enabled", true)?;
        handlers.insert("command", Handler { enabled, commands });
        futures.push(vars.run().boxed());
        Ok(())
    }
}
