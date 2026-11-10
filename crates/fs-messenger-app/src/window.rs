//! The window: one `TypeView` per data type inside `admin_window`.
//!
//! Pattern: Facade — wires descriptions, Backends and words into the generated frontend. No drawing code and no look: the
//! style comes from `fs-components-styles` (Admin), the screens from `fs-admin-frontend`.

use std::sync::Arc;

use fs_admin_frontend::view::{AboutInfo, AdminSurface};
use fs_admin_frontend::window::{TypeView, admin_window};
use fs_admin_frontend::{Backend, Session, SystemClock};
use fs_components_app::{AppError, AppWidget};
use fs_components_fields::{Look, Say};
use fs_components_styles::StyleId;
use fs_messenger_core::describe::registry;

use crate::shared::Shared;
use crate::words::Words;
use crate::{AccountBackend, ConversationBackend, DraftBackend, MessageBackend};

/// The title of the window.
pub const TITLE: &str = "FreeSynergy Messenger";
/// Where help lives.
const HELP_URL: &str = "https://github.com/FreeSynergy/fs-documentation/blob/main/docs/en/programs/apps/messenger/index.md";
const REPOSITORY: &str = "https://github.com/FreeSynergy/fs-messenger";

/// What the About page says.
#[must_use]
pub fn about() -> AboutInfo {
    AboutInfo {
        name: "FreeSynergy.Messenger".into(),
        version: format!("v{}", env!("CARGO_PKG_VERSION")),
        origin: REPOSITORY.into(),
        licence: env!("CARGO_PKG_LICENSE").into(),
        repository: REPOSITORY.into(),
    }
}

/// The Backend of a data type, by name.
fn backend_of(name: &str, shared: &Arc<Shared>) -> Option<Arc<dyn Backend>> {
    let shared = Arc::clone(shared);
    Some(match name {
        "Conversation" => Arc::new(ConversationBackend::new(shared)),
        "Message" => Arc::new(MessageBackend::new(shared)),
        "Account" => Arc::new(AccountBackend::new(shared)),
        "Draft" => Arc::new(DraftBackend::new(shared)),
        _ => return None,
    })
}

/// The window over `shared`: Conversation, Message, Draft, Account (in this order).
///
/// # Errors
/// [`AppError`] if the shell refuses the screens.
pub fn build_window(shared: &Arc<Shared>) -> Result<AppWidget, AppError> {
    let say: Arc<dyn Say> = Arc::new(Words::english());
    let types = registry().map_err(|p| AppError::Window(p.to_string()))?;
    let clock = Arc::new(SystemClock::new());
    let mut views = Vec::new();
    for name in ["Conversation", "Message", "Draft", "Account"] {
        let (Some(description), Some(backend)) = (types.get(name), backend_of(name, shared)) else {
            continue;
        };
        let label = description.label.clone();
        let session = Session::new(description.clone(), backend, clock.clone());
        let surface = AdminSurface::new(
            session,
            Look::new(StyleId::Admin, false),
            Arc::clone(&say),
            None,
        );
        views.push(TypeView {
            id: name.to_owned(),
            label,
            surface: Arc::new(surface),
        });
    }
    admin_window(TITLE, &say, views, about(), HELP_URL.to_owned())
}
