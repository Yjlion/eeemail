//! eeemail's email-client layer.
//!
//! Everything in this module is eeemail's own code, not forked from
//! `chatmail/core`. Keeping it here rather than scattered through upstream
//! files is what makes merging from upstream tractable; see
//! `docs/adr/0001-fork-chatmail-core.md` and `docs/fork-patches.md`.

pub mod backup;
pub mod blobcrypt;
pub mod compose;
pub mod ephemeral;
pub mod gating;
pub mod labels;
pub mod policy;
pub mod rawmime;
pub mod receipts;
pub mod recipients;
pub mod search;
pub mod tags;
pub mod threading;
pub mod vault;
