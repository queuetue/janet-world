@tool
extends Control

## Janet World editor dock — shows live connection status and world stats
## while a JanetWorldClient node is present in the edited scene.

const _POLL_INTERVAL := 0.5

var _client: Node = null
var _elapsed := 0.0

@onready var _status_label: Label = %StatusLabel
@onready var _stats_label:  Label = %StatsLabel
@onready var _refresh_btn:  Button = %RefreshButton


func _ready() -> void:
	_refresh_btn.pressed.connect(_scan_for_client)
	_scan_for_client()


func _process(delta: float) -> void:
	_elapsed += delta
	if _elapsed >= _POLL_INTERVAL:
		_elapsed = 0.0
		_refresh_display()


# --------------------------------------------------------------------------- #
#  Helpers
# --------------------------------------------------------------------------- #

func _scan_for_client() -> void:
	_client = null
	var root := EditorInterface.get_edited_scene_root() if Engine.is_editor_hint() else get_tree().root
	if root == null:
		_set_status("No scene open")
		return

	_client = _find_client(root)
	if _client == null:
		_set_status("No JanetWorldClient in scene")
	else:
		_set_status("Found: " + _client.name)
	_refresh_display()


func _find_client(node: Node) -> Node:
	if node.get_class() == "JanetWorldClient":
		return node
	for child in node.get_children():
		var result := _find_client(child)
		if result != null:
			return result
	return null


func _refresh_display() -> void:
	if _client == null:
		_stats_label.text = ""
		return

	# These methods are exported from the Rust node.
	var lines := PackedStringArray()
	if _client.has_method("is_connected_to_world"):
		lines.append("Connected: " + str(_client.is_connected_to_world()))
	if _client.has_method("last_frame"):
		lines.append("Frame: " + str(_client.last_frame()))
	if _client.has_method("active_chunk_count"):
		lines.append("Chunks: " + str(_client.active_chunk_count()))
	if _client.has_method("entity_count"):
		lines.append("Entities: " + str(_client.entity_count()))
	if _client.has_method("structure_count"):
		lines.append("Structures: " + str(_client.structure_count()))

	_stats_label.text = "\n".join(lines)


func _set_status(msg: String) -> void:
	_status_label.text = msg
