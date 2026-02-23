//! Janet World GDExtension for Godot 4
//!
//! Entry point.  Registers all GodotClass types with the engine.

pub mod bridge;
pub mod cache;
pub mod events;
pub mod node;

use godot::prelude::*;

struct JanetWorldLibrary;

/// GDExtension init hook — called by Godot on library load.
#[gdextension]
unsafe impl ExtensionLibrary for JanetWorldLibrary {}
