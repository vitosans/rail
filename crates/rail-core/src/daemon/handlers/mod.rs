//! Every operation the daemon serves.

mod account_validate;
mod actions;
mod attachment_image;
pub(super) mod attachments;
#[cfg(all(test, target_os = "macos"))]
mod attachments_quarantine_tests;
#[cfg(test)]
mod attachments_sweep_tests;
mod body;
pub(super) mod draft_files;
mod drafts;
mod drafts_gmail;
mod guide;
#[cfg(test)]
mod image_cache_tests;
pub(super) mod inline_refresh;
mod login;
mod passwords;
mod pending;
mod presets;
mod remote_image;
mod remove;
mod saved;
mod secrets;
mod snooze;
mod status;
mod sync;

use super::registry::Registry;
use crate::ops::op;

/// The registry with every handler.
pub(crate) fn registry() -> Registry {
    let mut registry = Registry::default();
    registry.register::<op::Hello, _, _>(status::hello);
    registry.register::<op::DaemonInfoOp, _, _>(status::daemon_info);
    registry.register::<op::DaemonStop, _, _>(status::daemon_stop);
    registry.register::<op::Status, _, _>(status::status);
    registry.register::<op::AccountsList, _, _>(status::accounts_list);
    registry.register::<op::SecretsConfigure, _, _>(secrets::configure);
    registry.register::<op::AccountAddBegin, _, _>(login::add_begin);
    registry.register::<op::AccountLoginBegin, _, _>(login::login_begin);
    registry.register::<op::ConsentBegin, _, _>(login::consent_begin);
    registry.register::<op::PendingWait, _, _>(pending::wait);
    registry.register::<op::PendingCancel, _, _>(pending::cancel);
    registry.register::<op::PresetsList, _, _>(presets::presets_list);
    registry.register::<op::PresetFetch, _, _>(presets::preset_fetch);
    registry.register::<op::AccountRemove, _, _>(remove::remove);
    registry.register::<op::GuideProvider, _, _>(guide::provider);
    registry.register::<op::SyncStatus, _, _>(sync::status);
    registry.register::<op::SyncNow, _, _>(sync::now);
    registry.register::<op::Search, _, _>(sync::search);
    registry.register::<op::BodyFetch, _, _>(body::fetch);
    registry.register::<op::ActionApply, _, _>(actions::apply);
    registry.register::<op::ActionUndo, _, _>(actions::undo);
    registry.register::<op::ActionRetry, _, _>(actions::retry);
    registry.register::<op::DraftSave, _, _>(drafts::save);
    registry.register::<op::DraftSend, _, _>(drafts::send);
    registry.register::<op::DraftDiscard, _, _>(drafts::discard);
    registry.register::<op::DraftPush, _, _>(drafts_gmail::push);
    registry.register::<op::DraftImport, _, _>(drafts_gmail::import);
    registry.register::<op::SnoozeSet, _, _>(snooze::set);
    registry.register::<op::SnoozeClear, _, _>(snooze::clear);
    registry.register::<op::AttachmentSave, _, _>(attachments::save);
    registry.register::<op::AttachmentOpen, _, _>(attachments::open);
    registry.register::<op::AttachmentImage, _, _>(attachment_image::image);
    registry.register::<op::ImageRemote, _, _>(remote_image::image);
    registry.register::<op::SavedSearchPut, _, _>(saved::put);
    registry.register::<op::SavedSearchRemove, _, _>(saved::remove);
    registry.register::<op::AccountValidate, _, _>(account_validate::validate);
    registry.register::<op::AccountPasswordSet, _, _>(passwords::set);
    registry.register::<op::AccountPasswordCheck, _, _>(passwords::check);
    registry.register::<op::AccountPasswordDelete, _, _>(passwords::delete);
    registry
}
