//! `:images`: draw an HTML body's pictures or leave them out, and `:remote-images`: fetch
//! remote pictures always or only once a thread's are loaded, each for the session and in
//! `config.toml`; `I`: always fetch one sender's remote pictures, or stop; and asking the daemon for the open messages' pictures, once each. Pictures
//! need rich mode and a terminal Rail could ask about its cells (`MailContext::picker`).

use std::collections::HashSet;

use super::MailScreen;
use super::themes::Saved;
use crate::tui::app::Effect;
use crate::tui::data::MailEvent;
use crate::tui::keys::Action;
use crate::tui::screens::reader::{DecodedPicture, PictureSource, Reader};
use crate::tui::text::escape;

impl MailScreen {
    /// Flip images (the open reader's too), say so, and save: `images = false` when off; on
    /// removes the key, since on is the default.
    pub(super) fn toggle_images(&mut self) -> Vec<Effect> {
        self.ctx.images = !self.ctx.images;
        let picker = self.picker();
        if let Some(reader) = &mut self.reader {
            reader.set_images(picker);
        }
        let (notice, value) = if self.ctx.images {
            ("images: on", None)
        } else {
            ("images: off", Some(false))
        };
        let mut effects = self.flip_and_save(
            Some(notice.to_string()),
            "images",
            Saved::Bool(value),
            |result| MailEvent::ImagesSaved { result },
        );
        effects.extend(self.load_pictures());
        effects
    }

    /// Flip remote pictures (the open reader's too, fetching its pictures when on), say so,
    /// and save: `remote_images = true` when on; off removes the key, since off is the default.
    pub(super) fn toggle_remote_images(&mut self) -> Vec<Effect> {
        self.ctx.remote_images = !self.ctx.remote_images;
        if let Some(reader) = &mut self.reader {
            reader.set_remote(self.ctx.remote_images);
        }
        let (notice, value) = if self.ctx.remote_images {
            ("remote images: on", Some(true))
        } else {
            ("remote images: off", None)
        };
        let mut effects = self.flip_and_save(
            Some(notice.to_string()),
            "remote_images",
            Saved::Bool(value),
            |result| MailEvent::RemoteImagesSaved { result },
        );
        effects.extend(self.load_pictures());
        effects
    }

    /// `I`: always load the cursor message's sender's remote pictures (fetching this thread's
    /// now, as `i` does, whether or not its sender was vouched for), or stop, say so, and add
    /// or remove just that address in `remote_images_from` as the file has it, so a sender
    /// added elsewhere stays (an empty list removes the key). Later mail loads by itself only
    /// when the receiving server vouched for the sender's domain.
    pub(super) fn trust_sender(&mut self) -> Vec<Effect> {
        let Some(address) = self.reader.as_ref().and_then(Reader::sender_at_cursor) else {
            return Vec::new();
        };
        let list = &mut self.ctx.remote_images_from;
        let found = list.iter().position(|a| a.to_lowercase() == address);
        let trusted = match found {
            Some(at) => {
                list.remove(at);
                false
            }
            None => {
                list.push(address.clone());
                true
            }
        };
        let senders = self.trusted_senders();
        if let Some(reader) = &mut self.reader {
            reader.set_trusted(senders);
            if trusted {
                reader.load_remote();
            }
        }
        let choice = if trusted { "always" } else { "ask first" };
        let notice = format!("remote pictures from {}: {choice}", escape(&address));
        let mut effects = self.flip_and_save(
            Some(notice),
            "remote_images_from",
            Saved::InList(address.clone(), trusted),
            |result| MailEvent::RemoteImagesSaved { result },
        );
        effects.extend(self.load_pictures());
        effects
    }

    /// `remote_images_from` in lower case, as the reader matches senders.
    pub(super) fn trusted_senders(&self) -> HashSet<String> {
        let list = &self.ctx.remote_images_from;
        list.iter().map(|a| a.to_lowercase()).collect()
    }

    /// What a body with remote pictures held back starts with: the keys as bound.
    pub(super) fn remote_hint(&self) -> String {
        let keys = &self.ctx.keymap;
        format!(
            "Remote pictures are not shown; {} loads them, {} always for this sender.",
            keys.label(Action::ReaderLoadImages),
            keys.label(Action::ReaderTrustImages),
        )
    }

    /// What a reader draws pictures with: none with images off or no terminal to ask.
    pub(super) fn picker(&self) -> Option<ratatui_image::picker::Picker> {
        self.ctx.picker.clone().filter(|_| self.ctx.images)
    }

    /// One `attachment.image` or `image.remote` per open message picture not asked for yet,
    /// as many as the reader has room for (`Reader::pictures_wanted`).
    pub(crate) fn load_pictures(&mut self) -> Vec<Effect> {
        let Some(reader) = self.reader.as_ref() else {
            return Vec::new();
        };
        let account = self
            .account_ids
            .iter()
            .position(|id| *id == Some(reader.account_id()))
            .and_then(|i| self.accounts.get(i))
            .map(|a| a.name.clone());
        let Some(account) = account else {
            return Vec::new();
        };
        let Some(reader) = self.reader.as_mut() else {
            return Vec::new();
        };
        let wanted = reader.pictures_wanted();
        reader.mark_pictures_asked(&wanted);
        wanted
            .into_iter()
            .map(|(id, remote, source)| self.ctx.link.picture(account.clone(), id, remote, source))
            .collect()
    }

    /// A picture read: the open reader's own messages only; then the next ones waiting.
    pub(super) fn picture_read(
        &mut self,
        message_id: i64,
        source: PictureSource,
        image: Option<DecodedPicture>,
    ) -> Vec<Effect> {
        if let Some(reader) = self
            .reader
            .as_mut()
            .filter(|r| r.detail().messages.iter().any(|m| m.id == message_id))
        {
            reader.picture_read(message_id, source, image);
        }
        self.load_pictures()
    }
}
