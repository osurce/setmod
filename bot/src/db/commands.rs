use crate::{db, template};
use diesel::prelude::*;
use failure::{format_err, ResultExt as _};
use hashbrown::{HashMap, HashSet};
use parking_lot::RwLock;
use std::{
    fmt,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

/// Local database wrapper.
#[derive(Clone)]
struct Database(db::Database);

impl Database {
    private_database_group_fns!(commands, Command, Key);

    fn edit(&self, key: &Key, text: &str) -> Result<Option<db::models::Command>, failure::Error> {
        use db::schema::commands::dsl;
        let c = self.0.pool.lock();

        let filter =
            dsl::commands.filter(dsl::channel.eq(&key.channel).and(dsl::name.eq(&key.name)));

        match filter
            .clone()
            .first::<db::models::Command>(&*c)
            .optional()?
        {
            None => {
                let command = db::models::Command {
                    channel: key.channel.to_string(),
                    name: key.name.to_string(),
                    count: 0,
                    text: text.to_string(),
                    group: None,
                    disabled: false,
                };

                diesel::insert_into(dsl::commands)
                    .values(&command)
                    .execute(&*c)?;
                return Ok(Some(command));
            }
            Some(command) => {
                let mut set = db::models::UpdateCommand::default();
                set.text = Some(text);
                diesel::update(filter).set(&set).execute(&*c)?;

                if command.disabled {
                    return Ok(None);
                }

                return Ok(Some(command));
            }
        }
    }

    fn increment(&self, key: &Key) -> Result<bool, failure::Error> {
        use db::schema::commands::dsl;

        let c = self.0.pool.lock();
        let count = diesel::update(
            dsl::commands.filter(dsl::channel.eq(&key.channel).and(dsl::name.eq(&key.name))),
        )
        .set(dsl::count.eq(dsl::count + 1))
        .execute(&*c)?;
        Ok(count == 1)
    }
}

#[derive(Clone)]
pub struct Commands {
    inner: Arc<RwLock<HashMap<Key, Arc<Command>>>>,
    db: Database,
}

impl Commands {
    database_group_fns!(Command, Key);

    /// Construct a new commands store with a db.
    pub fn load(db: db::Database) -> Result<Commands, failure::Error> {
        let db = Database(db);

        let mut inner = HashMap::new();

        for command in db.list()? {
            let command = Command::from_db(command)?;
            inner.insert(command.key.clone(), Arc::new(command));
        }

        Ok(Commands {
            inner: Arc::new(RwLock::new(inner)),
            db,
        })
    }

    /// Insert a word into the bad words list.
    pub fn edit(
        &self,
        channel: &str,
        name: &str,
        template: template::Template,
    ) -> Result<(), failure::Error> {
        let key = Key::new(channel, name);

        let mut inner = self.inner.write();

        if let Some(command) = self.db.edit(&key, template.source())? {
            let vars = template.vars();

            inner.insert(
                key.clone(),
                Arc::new(Command {
                    key,
                    count: Arc::new(AtomicUsize::new(command.count as usize)),
                    template,
                    vars,
                    group: command.group,
                    disabled: command.disabled,
                }),
            );
        } else {
            inner.remove(&key);
        }

        Ok(())
    }

    /// Increment the specified command.
    pub fn increment(&self, command: &Command) -> Result<(), failure::Error> {
        self.db.increment(&command.key)?;
        command.count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
pub struct Key {
    pub channel: String,
    pub name: String,
}

impl Key {
    pub fn new(channel: &str, name: &str) -> Self {
        Self {
            channel: channel.to_string(),
            name: name.to_lowercase(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Command {
    pub key: Key,
    #[serde(serialize_with = "serialize_count")]
    count: Arc<AtomicUsize>,
    pub template: template::Template,
    vars: HashSet<String>,
    pub group: Option<String>,
    pub disabled: bool,
}

/// Serialize the atomic count.
fn serialize_count<S>(value: &Arc<AtomicUsize>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::Serialize as _;

    value
        .load(std::sync::atomic::Ordering::SeqCst)
        .serialize(serializer)
}

impl Command {
    pub const NAME: &'static str = "command";

    /// Load a command from the database.
    pub fn from_db(command: db::models::Command) -> Result<Command, failure::Error> {
        let template = template::Template::compile(&command.text)
            .with_context(|_| format_err!("failed to compile command `{:?}` from db", command))?;

        let key = Key::new(command.channel.as_str(), command.name.as_str());
        let count = Arc::new(AtomicUsize::new(command.count as usize));
        let vars = template.vars();

        Ok(Command {
            key,
            count,
            template,
            vars,
            group: command.group,
            disabled: command.disabled,
        })
    }

    /// Get the currenct count.
    pub fn count(&self) -> i32 {
        self.count.load(Ordering::SeqCst) as i32
    }

    /// Render the given command.
    pub fn render<T>(&self, data: &T) -> Result<String, failure::Error>
    where
        T: serde::Serialize,
    {
        Ok(self.template.render_to_string(data)?)
    }

    /// Test if the rendered command has the given var.
    pub fn has_var(&self, var: &str) -> bool {
        self.vars.contains(var)
    }
}

impl fmt::Display for Command {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            fmt,
            "template = \"{template}\", group = {group}, disabled = {disabled}",
            template = self.template,
            group = self.group.as_ref().map(|g| g.as_str()).unwrap_or("*none*"),
            disabled = self.disabled,
        )
    }
}
