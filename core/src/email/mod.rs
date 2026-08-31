//! eeemail's email-client layer.
//!
//! Everything in this module is eeemail's own code, not forked from
//! `chatmail/core`. Keeping it here rather than scattered through upstream
//! files is what makes merging from upstream tractable; see
//! `docs/adr/0001-fork-chatmail-core.md` and `docs/fork-patches.md`.

pub mod rawmime;
