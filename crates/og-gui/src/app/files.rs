//! Files and folders dropped on the window or chosen, as inputs on the canvas, and where new
//! nodes go.

use super::*;

impl App {
    // ---------- Files and folders ----------

    /// How many files each reader of a folder stands for, by node id.
    pub(super) fn folder_counts(&mut self) -> HashMap<String, usize> {
        let base = self.base_dir();
        let mut out = HashMap::new();
        for n in self.snarl.nodes().filter(|n| n.kind == "Reader" && n.params.contains_key("path")) {
            self.lookup.find_sources(n, &base);
            if let Some(Ok(s)) = self.lookup.sources(&n.id) {
                if s.is_many() {
                    out.insert(n.id.clone(), s.files.len());
                }
            }
        }
        out
    }

    /// Adds a reader for a file or a folder, placed where there is room.
    pub(super) fn add_reader(&mut self, path: &Path, name: Option<String>, layers: &[String], at: Option<egui::Pos2>) -> NodeId {
        let pos = at.map(|p| self.free_spot_from(p)).unwrap_or_else(|| self.free_spot());
        let mut node = graph::new_node(&self.snarl, "Reader");
        let stem = if path.is_dir() { path.file_name() } else { path.file_stem() };
        if let Some(name) = name.or_else(|| stem.map(|s| s.to_string_lossy().into_owned())).filter(|s| !s.is_empty()) {
            node.name = name;
        }
        node.params.insert("path".into(), serde_json::Value::String(self.stored_path(path)));
        if !layers.is_empty() {
            node.params.insert("layer".into(), serde_json::Value::Array(layers.iter().cloned().map(serde_json::Value::String).collect()));
        }
        if path.is_dir() {
            // The filter starts as the kind of file there is most of in the folder.
            if let Some(pattern) = sources::guess_pattern(path).filter(|p| p != DEFAULT_PATTERN) {
                node.params.insert("pattern".into(), serde_json::Value::String(pattern));
            }
        }
        self.snarl.insert_node(pos, node)
    }

    /// Carries out the choice of layers for a file: one reader per layer, or one for all.
    pub fn add_layer_readers(&mut self, choice: &LayerChoice) {
        let names: Vec<String> = choice.layers.iter().zip(&choice.chosen).filter(|(_, on)| **on).map(|(l, _)| l.name.clone()).collect();
        let added: Vec<NodeId> = match names.as_slice() {
            [] => return,
            [one] => vec![self.add_reader(&choice.file, Some(one.clone()), std::slice::from_ref(one), choice.at)],
            many if choice.one_stream => vec![self.add_reader(&choice.file, None, many, choice.at)],
            many => many.iter().map(|l| self.add_reader(&choice.file, Some(l.clone()), std::slice::from_ref(l), choice.at)).collect(),
        };
        self.after_adding_readers(added);
    }

    pub(super) fn after_adding_readers(&mut self, added: Vec<NodeId>) {
        match added.len() {
            0 => {}
            1 => {
                let id = self.snarl[added[0]].id.clone();
                self.say(format!("Added an input for {}", self.snarl[added[0]].name));
                self.focus_node(&id);
                self.changed(true);
            }
            n => {
                self.say(format!("Added {} inputs", n));
                self.set_selection(added);
                self.changed(true);
            }
        }
    }

    /// The path as it is stored in a node. Relative to the workspace when it has been
    /// saved and the file is in its folder, so that the folder can be moved as a whole.
    pub(super) fn stored_path(&self, file: &Path) -> String {
        if self.path.is_some() {
            params::relative_to(&self.base_dir(), file)
        } else {
            file.to_string_lossy().into_owned()
        }
    }

    /// Takes files that were dropped on the window. A workspace is opened. A folder becomes
    /// one reader for the files in it. A file with several layers asks which ones to read.
    /// Anything else becomes a reader that points to the file.
    pub fn drop_files(&mut self, files: &[PathBuf]) {
        self.drop_files_at(files, None);
    }

    /// Like drop_files, with the place on the screen where the files were dropped. Inputs
    /// dropped on the canvas are put there, one below the other.
    pub fn drop_files_at(&mut self, files: &[PathBuf], at: Option<egui::Pos2>) {
        let at = at.filter(|p| self.canvas_rect.is_finite() && self.canvas_rect.contains(*p)).map(|p| self.transform.inverse() * p);
        let ext = |p: &Path| sources::ext_of(p);
        if let Some(ws) = files.iter().find(|p| (ext(p) == "ographis" || ext(p) == "json") && Workspace::load(p).is_ok()) {
            self.request(Pending::OpenPath(ws.clone()));
            return;
        }
        let is_sidecar = |p: &Path| {
            let e = ext(p);
            // A .dbf is the attribute table of a Shapefile when the .shp came along, and a
            // table of its own otherwise.
            let table_of_shp = e == "dbf" && files.iter().any(|o| ext(o) == "shp" && o.with_extension("") == p.with_extension(""));
            let metadata = e == "xml" && p.file_stem().is_some_and(|s| s.to_string_lossy().to_lowercase().ends_with(".shp"));
            !p.is_dir() && (SIDECARS.contains(&e.as_str()) || table_of_shp || metadata)
        };
        let wanted: Vec<&PathBuf> = files.iter().filter(|p| !is_sidecar(p)).collect();
        if wanted.is_empty() {
            self.say("Those files belong to a Shapefile. Drop the .shp file to read it.");
            return;
        }
        let mut added = Vec::new();
        for file in wanted {
            // The first goes where the pointer is, and the next below it.
            let next = at.map(|p| p + egui::vec2(0.0, 130.0 * (added.len() + self.layer_choices.len()) as f32));
            let is_folder = file.is_dir() && ext(file) != "gdb";
            if !is_folder {
                self.lookup.find_layers(&*self.backend, file);
                if let Some(Ok(layers)) = self.lookup.layers(file) {
                    if layers.len() > 1 {
                        let choice = LayerChoice { file: file.clone(), layers: layers.clone(), chosen: vec![false; layers.len()], one_stream: false, at: next };
                        self.layer_choices.push_back(choice);
                        continue;
                    }
                }
            }
            added.push(self.add_reader(file, None, &[], next));
        }
        self.after_adding_readers(added);
    }

    /// The first place from `p` and down where a new node covers no other node.
    pub(super) fn free_spot_from(&self, p: egui::Pos2) -> egui::Pos2 {
        let size = egui::vec2(190.0, 110.0);
        let taken: Vec<Rect> = self
            .snarl
            .nodes_pos_ids()
            .map(|(id, pos, _)| self.node_rects.iter().find(|r| r.0 == id).map_or(Rect::from_min_size(pos, size), |r| r.1).expand(10.0))
            .collect();
        let mut spot = p;
        for _ in 0..400 {
            if !taken.iter().any(|t| t.intersects(Rect::from_min_size(spot, size))) {
                return spot;
            }
            spot.y += 20.0;
        }
        p
    }

    /// A free place for a new node: closest to the middle of the visible area without
    /// covering another node.
    pub(super) fn free_spot(&self) -> egui::Pos2 {
        if !self.canvas_rect.is_finite() {
            return egui::pos2(40.0, 40.0);
        }
        let to_graph = self.transform.inverse();
        let view = Rect::from_two_pos(to_graph * self.canvas_rect.min, to_graph * self.canvas_rect.max);
        let size = egui::vec2(190.0, 120.0);
        let taken: Vec<Rect> = self
            .snarl
            .nodes_pos_ids()
            .map(|(id, pos, _)| self.node_rects.iter().find(|r| r.0 == id).map_or(Rect::from_min_size(pos, size), |r| r.1).expand(14.0))
            .collect();
        let center = view.center() - size / 2.0;
        let mut best: Option<(f32, egui::Pos2)> = None;
        let mut y = view.top() + 16.0;
        while y + size.y <= view.bottom() {
            let mut x = view.left() + 16.0;
            while x + size.x <= view.right() {
                let spot = Rect::from_min_size(egui::pos2(x, y), size);
                if !taken.iter().any(|t| t.intersects(spot)) {
                    let d = (x - center.x).abs() + (y - center.y).abs() * 1.5;
                    if best.is_none_or(|b| d < b.0) {
                        best = Some((d, spot.min));
                    }
                }
                x += 24.0;
            }
            y += 24.0;
        }
        // If there is no gap in the visible area the node goes below everything else.
        best.map(|b| b.1).unwrap_or_else(|| egui::pos2(view.left() + 16.0, self.bounds().map_or(40.0, |b| b.bottom() + 24.0)))
    }

    /// The rectangle of the node on the canvas, in graph coordinates. Available once the
    /// node has been drawn.
    pub fn node_rect(&self, id: &str) -> Option<Rect> {
        let node = graph::find(&self.snarl, id)?;
        self.node_rects.iter().find(|r| r.0 == node).map(|r| r.1)
    }

    pub(super) fn bounds(&self) -> Option<Rect> {
        let mut r: Option<Rect> = None;
        for (id, pos, _) in self.snarl.nodes_pos_ids() {
            // The drawn size if there is one, otherwise an estimate.
            let node = self.node_rects.iter().find(|x| x.0 == id).map_or(Rect::from_min_size(pos, egui::vec2(190.0, 110.0)), |x| x.1);
            r = Some(r.map_or(node, |x| x.union(node)));
        }
        for g in &self.groups {
            r = Some(r.map_or(group_rect(g), |x| x.union(group_rect(g))));
        }
        r
    }

    /// Moves the canvas so that the node can be seen, if it cannot already.
    pub(super) fn reveal(&mut self, id: &str) {
        let Some(rect) = self.node_rect(id) else { return };
        if self.canvas_rect.is_finite() && !self.canvas_rect.shrink(8.0).contains_rect(self.transform * rect) {
            let scale = self.transform.scaling;
            self.fit = Some(TSTransform::new(self.canvas_rect.center().to_vec2() - rect.center().to_vec2() * scale, scale));
        }
    }

    /// What there is to show for the chosen node: (port name, table) and the chosen port.
    pub(super) fn viewed(&self) -> Option<Viewed> {
        let (id, result) = (self.view.node.as_deref()?, self.result.as_ref()?);
        let r = result.nodes.get(id)?;
        let node = self.snarl.nodes().find(|n| n.id == id)?;
        node.ty()?;
        // A writer has no output ports. What came in is shown there. A View shows each
        // stream connected to it, in the order of the connections.
        let outputs = node.outputs();
        let ports: Vec<(String, Arc<Table>)> = if node.kind == "Viewer" {
            let mut list: Vec<(String, Arc<Table>)> = r.outputs.iter().map(|(k, t)| (k.clone(), t.clone())).collect();
            list.sort_by_key(|(k, _)| k.split(' ').next().and_then(|n| n.parse::<usize>().ok()).unwrap_or(usize::MAX));
            list
        } else if outputs.is_empty() {
            node.inputs().iter().filter_map(|p| r.inputs.get(p).map(|t| (format!("{} (in)", p), t.clone()))).collect()
        } else {
            outputs.iter().filter_map(|p| r.outputs.get(p).map(|t| (p.clone(), t.clone()))).collect()
        };
        if ports.is_empty() {
            return None;
        }
        let chosen =
            self.view.port.as_deref().and_then(|p| ports.iter().position(|x| x.0 == p)).or_else(|| ports.iter().position(|x| !x.1.is_empty())).unwrap_or(0);
        Some((ports, chosen))
    }
}

/// Where the pointer is in the window when files are dropped on it. While files are
/// dragged over it, Windows sends the window no pointer movements, so there the place is
/// asked of the system.
pub(super) fn drop_position(ctx: &egui::Context) -> Option<egui::Pos2> {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct Point {
            x: i32,
            y: i32,
        }
        #[link(name = "user32")]
        unsafe extern "system" {
            fn GetCursorPos(point: *mut Point) -> i32;
        }
        let mut pt = Point { x: 0, y: 0 };
        // SAFETY: GetCursorPos writes the position into the struct it is given.
        if unsafe { GetCursorPos(&mut pt) } != 0 {
            let (inner, ppp) = ctx.input(|i| (i.viewport().inner_rect, i.pixels_per_point));
            if let Some(inner) = inner {
                return Some(egui::pos2(pt.x as f32 / ppp, pt.y as f32 / ppp) - inner.min.to_vec2());
            }
        }
    }
    ctx.input(|i| i.pointer.latest_pos())
}
