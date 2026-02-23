## Thin GDScript hint so the editor can display helpful tooltips before the
## GDExtension is fully registered.  At runtime this script is never used —
## the Rust-native JanetWorldClient class takes over.
##
## DO NOT add actual logic here.
@tool
extends Node
