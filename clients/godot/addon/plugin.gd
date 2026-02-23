@tool
extends EditorPlugin

## Janet World editor plugin.
##
## Loads the GDExtension and adds the editor dock when the plugin is enabled.
## Unloads cleanly on disable.

const DOCK_SCENE := preload("res://addons/janet_world/dock.tscn")

var _dock: Control = null


func _enter_tree() -> void:
	_dock = DOCK_SCENE.instantiate()
	add_control_to_dock(DOCK_SLOT_RIGHT_BL, _dock)
	add_custom_type(
		"JanetWorldClient",
		"Node",
		preload("res://addons/janet_world/JanetWorldClientHint.gd"),
		preload("res://addons/janet_world/icon.svg")
	)


func _exit_tree() -> void:
	if _dock:
		remove_control_from_docks(_dock)
		_dock.queue_free()
		_dock = null
	remove_custom_type("JanetWorldClient")


func _get_plugin_name() -> String:
	return "Janet World"
