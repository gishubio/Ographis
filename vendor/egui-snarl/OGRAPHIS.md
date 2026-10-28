# egui-snarl, as used by Ographis

This is [egui-snarl](https://github.com/zakarumych/egui-snarl) 0.12.0 by Zakarum, licensed
MIT OR Apache-2.0 (see `LICENSE-MIT` and `LICENSE-APACHE`). Examples and images are left
out. The source is unchanged except for the places marked `Ographis:` in `src/ui.rs` and
`src/ui/state.rs`, `src/ui/viewer.rs` and `src/ui/wire.rs`:

| Change | Upstream behaviour |
| --- | --- |
| Dragging on empty canvas draws a selection box | Needs Shift. A plain drag pans |
| The selection box starts where the button was pressed | It starts where the drag was recognised, a few pixels later |
| The canvas is panned with the middle button, or Space + drag | Primary button |
| The wheel zooms at the pointer | The wheel pans, Ctrl + wheel zooms |
| A click on empty canvas clears the selection | Needs Ctrl |
| A click on a node selects just that node | Needs Shift |
| `set_selected_nodes` sets the selection from outside | Not available |
| `SnarlViewer::show_background` lets the viewer add widgets under the nodes | Not available. `draw_background` only gets a painter |
| A node dragged over a connection can be put into it: `can_drop_node_on_wire` and `drop_node_on_wire` | Not available |
| A click on a connection is reported through `wire_clicked`, and `wire_stroke` decides how each connection is drawn | A connection can only be removed, with the secondary button |
| `wire_dashed` lets the viewer draw a connection in dashes | Not available |
| `wire_points` gives the points a connection is drawn through, as a smooth curve; `wire_point_moved` and `wire_point_secondary_clicked` tell the viewer what was done with them | Not available |
| A connection is hit within 5 points of it | 2 points |
| A pin is hit 6 points outside its shape | Only on its shape |
| Dragging from a connected input picks its connection up, to drop it on another input | Needs Ctrl |
| A click with the secondary button on a connection goes to `wire_secondary_clicked`, which by default removes it | Removes it |
| `wire.rs` no longer imports the module `core::f32` | The import made `f32::NAN` mean a constant that newer Rust compilers warn about |
| A node dragged near the edge of the canvas pans it that way, and the dragged nodes go along; the edge is that of the canvas as seen on the screen | The node stops at the edge of what is seen |
| The room for an output pin is taken through the layout, from right to left | Only the first output got the room, and the names of the others ran in under their pins |
| `wire_flow` of the viewer: dots that move along a connection, such as those into a node at work | No such hook |

The patch is wired in through `[patch.crates-io]` in the workspace `Cargo.toml`. To move to
a newer egui-snarl, replace `src/` and apply the changes again.
