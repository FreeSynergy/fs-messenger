//! The shapes of the capability table: [`In`] (one input or result field), [`Delivery`] and [`Entry`].
//!
//! Pattern: Value objects with `const` Builder-style methods, so the table in `table.rs` is a plain `static` that the
//! description, the Backend and the tests all read.

use fs_types::{BasicType, CommandKind};

/// One input or result field of a command in the table.
#[derive(Clone, Copy, Debug)]
pub struct In {
    /// Machine name, lower camel case.
    pub name: &'static str,
    /// The basic type of each value.
    pub basic: BasicType,
    /// A value is needed.
    pub required: bool,
    /// Several values are allowed.
    pub multiple: bool,
    /// For a `reference`: the data type referred to.
    pub target: &'static str,
    /// For a `choice`: the allowed keys.
    pub choices: &'static [&'static str],
}

impl In {
    /// An optional single field of `basic`.
    pub const fn new(name: &'static str, basic: BasicType) -> Self {
        Self {
            name,
            basic,
            required: false,
            multiple: false,
            target: "",
            choices: &[],
        }
    }
    /// A one-line text.
    pub const fn text(name: &'static str) -> Self {
        Self::new(name, BasicType::Text)
    }
    /// Several lines of text.
    pub const fn long(name: &'static str) -> Self {
        Self::new(name, BasicType::MultilineText)
    }
    /// A reference to a data type.
    pub const fn to(name: &'static str, target: &'static str) -> Self {
        let mut f = Self::new(name, BasicType::Reference);
        f.target = target;
        f
    }
    /// A choice of fixed keys.
    pub const fn pick(name: &'static str, choices: &'static [&'static str]) -> Self {
        let mut f = Self::new(name, BasicType::Choice);
        f.choices = choices;
        f
    }
    /// A value is needed.
    #[must_use]
    pub const fn req(mut self) -> Self {
        self.required = true;
        self
    }
    /// Several values are allowed.
    #[must_use]
    pub const fn many(mut self) -> Self {
        self.multiple = true;
        self
    }
}

/// How a capability reaches the person.
#[derive(Clone, Copy, Debug)]
pub enum Delivery {
    /// A command of the data type.
    Command {
        /// Command name, unique in its type; entries with one name describe one command.
        name: &'static str,
        /// What it does to the data.
        kind: CommandKind,
        /// The input, in the order of the call.
        input: &'static [In],
        /// The result.
        result: &'static [In],
    },
    /// A field of the data type that shows it (no command is needed).
    Field(&'static str),
}

/// One row of the table: a capability id of `requirements.md` (or of the core) and what the program does with it.
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    /// The capability id, `message.send`.
    pub capability: &'static str,
    /// The data type that carries it: `Conversation`, `Message`, `Account` or `Draft`.
    pub data_type: &'static str,
    /// Command or field.
    pub delivery: Delivery,
    /// The core can do it and the Backend carries it out.
    pub supported: bool,
    /// Why not, when `supported` is false (English, for DESIGN and logs; a person sees the label's translation).
    pub reason: &'static str,
}

impl Entry {
    /// The command name, if this row is a command.
    pub const fn command_name(&self) -> Option<&'static str> {
        match self.delivery {
            Delivery::Command { name, .. } => Some(name),
            Delivery::Field(_) => None,
        }
    }
}

/// A row that is a command the core carries out.
pub const fn done(
    capability: &'static str,
    data_type: &'static str,
    name: &'static str,
    kind: CommandKind,
    input: &'static [In],
    result: &'static [In],
) -> Entry {
    Entry {
        capability,
        data_type,
        delivery: Delivery::Command {
            name,
            kind,
            input,
            result,
        },
        supported: true,
        reason: "",
    }
}

/// A row that is a command the core cannot carry out (yet), with the reason.
pub const fn missing(
    capability: &'static str,
    data_type: &'static str,
    name: &'static str,
    kind: CommandKind,
    input: &'static [In],
    reason: &'static str,
) -> Entry {
    Entry {
        capability,
        data_type,
        delivery: Delivery::Command {
            name,
            kind,
            input,
            result: &[],
        },
        supported: false,
        reason,
    }
}

/// A row shown by a field of the type.
pub const fn shown(
    capability: &'static str,
    data_type: &'static str,
    field: &'static str,
) -> Entry {
    Entry {
        capability,
        data_type,
        delivery: Delivery::Field(field),
        supported: true,
        reason: "",
    }
}
