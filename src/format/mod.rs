//! tmux format-string handling: build a variable context from zellij state and
//! render `#{...}` templates against it.

pub mod context;
pub mod render;
