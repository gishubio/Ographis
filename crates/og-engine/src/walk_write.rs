//! A walk-through in the browser: a page with three.js where one walks at the height of the
//! eyes among buildings and objects of BIM, follows the ground and does not go through
//! walls, can fly, clicks an object for its attributes, and sees the sun and the shadows at a
//! time chosen. The model is inside the page, so that it opens from the disk as it is; three.js
//! comes from unpkg.

use crate::mesh_write::{write_glb_with, GltfOpts};
use crate::model::{GbError, Result, Table};
use base64::Engine;
use std::path::{Path, PathBuf};

/// How the walk begins and what it shows.
#[derive(Debug, Clone, Default)]
pub struct WalkOpts {
    pub title: String,
    /// The place of the origin in degrees, for the sun: latitude and longitude.
    pub latlon: Option<(f64, f64)>,
    /// The date and time, such as 2026-06-21 15:00, and the hours from UTC.
    pub when: String,
    pub utc_offset: f64,
    pub eye: f64,
    pub speed: f64,
    /// Where to begin, in the coordinates of the model. None: at the south edge, looking north.
    pub start: Option<[f64; 2]>,
}

const THREE: &str = "https://unpkg.com/three@0.170.0";

/// Writes the walk-through: index.html with the model in it, and the model as scene.glb.
/// The model is written around the origin of `gltf`, or near its middle; the sun and the
/// camera of glTF are left out, since the page has its own.
pub fn write_walk(table: &Table, folder: &Path, gltf: &GltfOpts, o: &WalkOpts) -> Result<Vec<PathBuf>> {
    let mesh = table.mesh.as_deref().ok_or_else(|| {
        GbError(
            "Nothing came in with a 3D model to walk in. Make one first, such as with Extrude, Make a diorama or a 3D input, or read a city model or IFC."
                .into(),
        )
    })?;
    std::fs::create_dir_all(folder).map_err(|e| GbError(format!("Cannot create the folder {}: {}", folder.display(), e)))?;
    let origin = gltf.origin.unwrap_or_else(|| crate::mesh_write::origin_of(mesh));
    let gltf = GltfOpts { sun: None, camera: false, origin: Some(origin), ..gltf.clone() };
    let glb = folder.join("scene.glb");
    write_glb_with(table, &glb, &gltf)?;
    let o = WalkOpts { start: o.start.map(|[x, y]| [x - origin[0], y - origin[1]]), ..o.clone() };
    let o = &o;
    let bytes = std::fs::read(&glb).map_err(|e| GbError(format!("Cannot read {}: {}", glb.display(), e)))?;
    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let html = page(o, &data);
    let index = folder.join("index.html");
    std::fs::write(&index, html).map_err(|e| GbError(format!("Cannot write {}: {}", index.display(), e)))?;
    Ok(vec![index, glb])
}

fn js_text(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

fn page(o: &WalkOpts, data: &str) -> String {
    let (lat, lon) = o.latlon.unwrap_or((f64::NAN, f64::NAN));
    let start = match o.start {
        Some([x, y]) => format!("[{}, {}]", x, y),
        None => "null".into(),
    };
    let title = if o.title.is_empty() { "Walk-through".to_string() } else { o.title.clone() };
    let esc = title.replace('&', "&amp;").replace('<', "&lt;");
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{esc}</title>
<style>
  html, body {{ margin: 0; height: 100%; overflow: hidden; background: #cfe3f2; font: 14px system-ui, sans-serif; }}
  canvas {{ display: block; }}
  #panel {{ position: absolute; left: 12px; top: 12px; background: rgba(255,255,255,0.92); padding: 10px 12px; border-radius: 8px; max-width: 320px; box-shadow: 0 1px 6px rgba(0,0,0,0.2); }}
  #panel h1 {{ font-size: 15px; margin: 0 0 6px; }}
  #panel p {{ margin: 4px 0; color: #333; }}
  #panel label {{ display: block; margin-top: 6px; }}
  #info {{ position: absolute; right: 12px; top: 12px; background: rgba(255,255,255,0.95); padding: 10px 12px; border-radius: 8px; max-width: 360px; max-height: 70vh; overflow: auto; display: none; box-shadow: 0 1px 6px rgba(0,0,0,0.2); }}
  #info table {{ border-collapse: collapse; }}
  #info td {{ padding: 2px 6px; border-bottom: 1px solid #eee; vertical-align: top; }}
  #info td:first-child {{ color: #555; }}
  #cross {{ position: absolute; left: 50%; top: 50%; width: 14px; height: 14px; margin: -7px 0 0 -7px; display: none; }}
  #cross:before, #cross:after {{ content: ""; position: absolute; background: #fff; box-shadow: 0 0 2px #000; }}
  #cross:before {{ left: 6px; top: 0; width: 2px; height: 14px; }}
  #cross:after {{ top: 6px; left: 0; height: 2px; width: 14px; }}
  #mode {{ font-weight: 600; }}
</style>
<script type="importmap">{{ "imports": {{ "three": "{three}/build/three.module.js", "three/addons/": "{three}/examples/jsm/" }} }}</script>
</head>
<body>
<div id="panel">
  <h1>{esc}</h1>
  <p>Click to look around. <b>W A S D</b> or the arrows to walk, <b>Shift</b> to run, the <b>wheel</b> or <b>+</b> and <b>−</b> for the speed, <b>F</b> to fly, <b>Space</b> and <b>C</b> up and down when flying, <b>E</b> or a click on what is in the middle for its attributes, <b>Esc</b> to stop.</p>
  <p>Mode: <span id="mode">walking</span>, <span id="speed"></span></p>
  <label>Date <input id="date" type="date"></label>
  <label>Time <input id="time" type="range" min="0" max="24" step="0.25"> <span id="clock"></span></label>
</div>
<div id="info"></div>
<div id="cross"></div>
<script type="module">
import * as THREE from "three";
import {{ GLTFLoader }} from "three/addons/loaders/GLTFLoader.js";
import {{ MeshoptDecoder }} from "three/addons/libs/meshopt_decoder.module.js";

const LAT = {lat}, LON = {lon}, UTC_OFFSET = {utc};
const EYE = {eye}, SPEED = {speed}, START = {start};
const WHEN = {when};
const DATA = "{data}";

const renderer = new THREE.WebGLRenderer({{ antialias: true }});
renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
renderer.setSize(window.innerWidth, window.innerHeight);
renderer.shadowMap.enabled = true;
renderer.shadowMap.type = THREE.PCFSoftShadowMap;
document.body.appendChild(renderer.domElement);
const scene = new THREE.Scene();
scene.background = new THREE.Color(0xcfe3f2);
const camera = new THREE.PerspectiveCamera(70, window.innerWidth / window.innerHeight, 0.1, 20000);
camera.rotation.order = "YXZ";
scene.add(new THREE.HemisphereLight(0xdfeeff, 0x7a6a55, 0.9));
const sun = new THREE.DirectionalLight(0xfff4e0, 2.2);
sun.castShadow = true;
sun.shadow.mapSize.set(4096, 4096);
sun.shadow.bias = -0.0005;
sun.shadow.normalBias = 0.05;
scene.add(sun, sun.target);

// The sun at a time and place: its azimuth clockwise from north and its altitude, in degrees.
function sunAt(date, lat, lon) {{
  const rad = Math.PI / 180;
  const d = date.getTime() / 86400000 - 10957.5;
  const g = (357.529 + 0.98560028 * d) * rad;
  const q = 280.459 + 0.98564736 * d;
  const L = (q + 1.915 * Math.sin(g) + 0.020 * Math.sin(2 * g)) * rad;
  const e = (23.439 - 0.00000036 * d) * rad;
  const ra = Math.atan2(Math.cos(e) * Math.sin(L), Math.cos(L));
  const dec = Math.asin(Math.sin(e) * Math.sin(L));
  const gmst = ((18.697374558 + 24.06570982441908 * d) % 24 + 24) % 24;
  const ha = (gmst * 15 + lon) * rad - ra;
  const alt = Math.asin(Math.sin(lat * rad) * Math.sin(dec) + Math.cos(lat * rad) * Math.cos(dec) * Math.cos(ha));
  const az = Math.atan2(-Math.sin(ha), Math.tan(dec) * Math.cos(lat * rad) - Math.sin(lat * rad) * Math.cos(ha));
  return {{ azimuth: (az / rad + 360) % 360, altitude: alt / rad }};
}}

const dateInput = document.getElementById("date"), timeInput = document.getElementById("time"), clock = document.getElementById("clock");
const [d0, t0] = WHEN.split(/[ T]/);
dateInput.value = d0 || "2026-06-21";
const [hh, mm] = (t0 || "15:00").split(":").map(Number);
timeInput.value = hh + (mm || 0) / 60;
let box = new THREE.Box3();
function placeSun() {{
  const h = Number(timeInput.value);
  clock.textContent = String(Math.floor(h)).padStart(2, "0") + ":" + String(Math.round((h % 1) * 60)).padStart(2, "0");
  const lat = isNaN(LAT) ? 57.7 : LAT, lon = isNaN(LON) ? 12.0 : LON;
  const utc = new Date(dateInput.value + "T00:00:00Z").getTime() + (h - UTC_OFFSET) * 3600000;
  const s = sunAt(new Date(utc), lat, lon);
  const a = s.azimuth * Math.PI / 180, alt = Math.max(s.altitude, -5) * Math.PI / 180;
  // East is x, up is y, north is -z.
  const dir = new THREE.Vector3(Math.sin(a) * Math.cos(alt), Math.sin(alt), -Math.cos(a) * Math.cos(alt));
  const c = box.getCenter(new THREE.Vector3()), r = Math.max(box.getSize(new THREE.Vector3()).length() / 2, 10);
  sun.position.copy(c).addScaledVector(dir, r * 2);
  sun.target.position.copy(c);
  const cam = sun.shadow.camera;
  cam.left = -r; cam.right = r; cam.top = r; cam.bottom = -r; cam.near = 0.1; cam.far = r * 4;
  cam.updateProjectionMatrix();
  sun.intensity = s.altitude > 0 ? 2.2 : 0.05;
}}
dateInput.addEventListener("input", placeSun);
timeInput.addEventListener("input", placeSun);

// The model, from inside the page.
const bytes = Uint8Array.from(atob(DATA), (ch) => ch.charCodeAt(0));
const solids = [];
new GLTFLoader().setMeshoptDecoder(MeshoptDecoder).parse(bytes.buffer, "", (gltf) => {{
  scene.add(gltf.scene);
  gltf.scene.traverse((o) => {{
    if (o.isMesh) {{ o.castShadow = true; o.receiveShadow = true; solids.push(o); }}
  }});
  box = new THREE.Box3().setFromObject(gltf.scene);
  const c = box.getCenter(new THREE.Vector3());
  if (START) {{ camera.position.set(START[0], box.max.y + 2, -START[1]); }}
  else {{ camera.position.set(c.x, box.max.y + 2, box.max.z - Math.min(5, (box.max.z - box.min.z) / 10)); }}
  const g = ground(camera.position);
  camera.position.y = (g === null ? box.min.y : g) + EYE;
  placeSun();
  showSpeed();
  window.ready = true;
}}, (e) => {{ document.getElementById("panel").insertAdjacentHTML("beforeend", "<p>The model could not be read: " + e + "</p>"); }});

const down = new THREE.Raycaster(), ahead = new THREE.Raycaster(), pick = new THREE.Raycaster();
// The height of the ground under a place: the highest surface below the knees.
function ground(p) {{
  down.set(new THREE.Vector3(p.x, p.y - EYE + 0.5, p.z), new THREE.Vector3(0, -1, 0));
  down.far = 1000;
  const hit = down.intersectObjects(solids, false)[0];
  return hit ? hit.point.y : null;
}}
// Whether a wall stands within reach in a direction, at the knees or the chest.
function blocked(p, dir, dist) {{
  for (const up of [0.5, 1.4]) {{
    ahead.set(new THREE.Vector3(p.x, p.y - EYE + up, p.z), dir);
    ahead.far = dist + 0.35;
    if (ahead.intersectObjects(solids, false).length) return true;
  }}
  return false;
}}

const keys = {{}};
let flying = false, fall = 0;
// The speed is a factor of the one given, by the wheel or + and −, remembered in this browser.
let factor = 1;
try {{ factor = Number(localStorage.getItem("walk-speed")) || 1; }} catch (e) {{}}
// Flying goes faster in a large model: across a fortieth of it a second, at least.
function flySpeed() {{ return Math.max(SPEED * 5, box.getSize(new THREE.Vector3()).length() / 40); }}
function showSpeed() {{
  const v = (flying ? flySpeed() : SPEED) * factor;
  document.getElementById("speed").textContent = (v < 10 ? v.toFixed(1) : Math.round(v)) + " m/s";
}}
function changeSpeed(by) {{
  factor = Math.min(64, Math.max(0.25, factor * by));
  try {{ localStorage.setItem("walk-speed", String(factor)); }} catch (e) {{}}
  showSpeed();
}}
addEventListener("wheel", (e) => {{ changeSpeed(e.deltaY < 0 ? 1.25 : 0.8); }}, {{ passive: true }});
addEventListener("keydown", (e) => {{
  keys[e.code] = true;
  if (e.key === "+" || e.code === "NumpadAdd" || e.code === "Equal") changeSpeed(1.25);
  if (e.key === "-" || e.code === "NumpadSubtract" || e.code === "Minus") changeSpeed(0.8);
  if (e.code === "KeyF") {{ flying = !flying; document.getElementById("mode").textContent = flying ? "flying" : "walking"; showSpeed(); }}
  if (e.code === "KeyE") showPicked();
}});
addEventListener("keyup", (e) => {{ keys[e.code] = false; }});
renderer.domElement.addEventListener("click", () => {{
  if (document.pointerLockElement) showPicked(); else renderer.domElement.requestPointerLock();
}});
document.addEventListener("pointerlockchange", () => {{
  document.getElementById("cross").style.display = document.pointerLockElement ? "block" : "none";
}});
addEventListener("mousemove", (e) => {{
  if (document.pointerLockElement !== renderer.domElement) return;
  camera.rotation.y -= e.movementX * 0.0025;
  camera.rotation.x = Math.max(-1.5, Math.min(1.5, camera.rotation.x - e.movementY * 0.0025));
}});
addEventListener("resize", () => {{
  camera.aspect = innerWidth / innerHeight; camera.updateProjectionMatrix(); renderer.setSize(innerWidth, innerHeight);
}});

// The attributes of what is in the middle of the view.
function showPicked() {{
  pick.setFromCamera(new THREE.Vector2(0, 0), camera);
  const hit = pick.intersectObjects(solids, false)[0];
  const info = document.getElementById("info");
  if (!hit) {{ info.style.display = "none"; return; }}
  let o = hit.object;
  while (o && !(o.userData && Object.keys(o.userData).length) && o.parent) o = o.parent;
  const data = (o && o.userData) || {{}};
  let rows = "<b>" + (o && o.name ? o.name : "Object") + "</b><table>";
  for (const [k, v] of Object.entries(data)) rows += "<tr><td>" + k + "</td><td>" + String(v).replace(/</g, "&lt;") + "</td></tr>";
  info.innerHTML = rows + "</table>";
  info.style.display = "block";
  window.picked = data;
}}
window.showPicked = showPicked;

const clockTime = new THREE.Clock();
function step() {{
  // Slow frames still move as far as the time that went, up to a quarter of a second.
  const dt = Math.min(clockTime.getDelta(), 0.25);
  const run = keys.ShiftLeft || keys.ShiftRight ? 4 : 1;
  const speed = (flying ? flySpeed() : SPEED) * factor;
  const forward = new THREE.Vector3(-Math.sin(camera.rotation.y), 0, -Math.cos(camera.rotation.y));
  const right = new THREE.Vector3(-forward.z, 0, forward.x);
  const move = new THREE.Vector3();
  if (keys.KeyW || keys.ArrowUp) move.add(forward);
  if (keys.KeyS || keys.ArrowDown) move.sub(forward);
  if (keys.KeyD || keys.ArrowRight) move.add(right);
  if (keys.KeyA || keys.ArrowLeft) move.sub(right);
  if (move.lengthSq() > 0) {{
    move.normalize().multiplyScalar(speed * run * dt);
    // Each way by itself, so that one slides along a wall.
    for (const axis of [new THREE.Vector3(move.x, 0, 0), new THREE.Vector3(0, 0, move.z)]) {{
      const len = axis.length();
      if (len === 0) continue;
      if (flying || !blocked(camera.position, axis.clone().normalize(), len)) camera.position.add(axis);
    }}
  }}
  if (flying) {{
    if (keys.Space) camera.position.y += speed * run * dt;
    if (keys.KeyC) camera.position.y -= speed * run * dt;
  }} else {{
    const g = ground(camera.position);
    const target = (g === null ? box.min.y : g) + EYE;
    if (camera.position.y > target + 0.05) {{ fall += 9.8 * dt; camera.position.y = Math.max(target, camera.position.y - fall * dt); }}
    else {{ fall = 0; camera.position.y = target; }}
  }}
  renderer.render(scene, camera);
  requestAnimationFrame(step);
}}
window.walk = {{ scene, camera, keys, ground, blocked, sunAt, placeSun }};
step();
</script>
</body>
</html>
"##,
        esc = esc,
        three = THREE,
        lat = if lat.is_finite() { lat.to_string() } else { "NaN".into() },
        lon = if lon.is_finite() { lon.to_string() } else { "NaN".into() },
        utc = o.utc_offset,
        eye = if o.eye > 0.0 { o.eye } else { 1.7 },
        speed = if o.speed > 0.0 { o.speed } else { 3.0 },
        start = start,
        when = js_text(&o.when),
        data = data
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sun_and_the_model_are_in_the_page() {
        let o =
            WalkOpts { title: "Skövde <centre>".into(), latlon: Some((58.39, 13.85)), when: "2026-06-21 15:00".into(), utc_offset: 2.0, ..Default::default() };
        let html = page(&o, "AAAA");
        assert!(html.contains("const LAT = 58.39, LON = 13.85, UTC_OFFSET = 2;"));
        assert!(html.contains("const DATA = \"AAAA\";"));
        assert!(html.contains("<title>Skövde &lt;centre></title>"));
        assert!(html.contains("three@0.170.0/examples/jsm/"));
    }

    #[test]
    fn the_walk_is_a_page_with_the_model_inside() {
        use crate::mesh::{Material, Mesh, Tri, NONE};
        let positions = vec![[418300.0, 6472550.0, 80.0], [418310.0, 6472550.0, 80.0], [418310.0, 6472560.0, 80.0]];
        let tris = vec![Tri { v: [0, 1, 2], n: [NONE; 3], t: [NONE; 3], material: 0, object: 0 }];
        let mesh = Mesh { positions, normals: Vec::new(), uvs: Vec::new(), tris, materials: vec![Material::default()], objects: vec!["Ground".into()] };
        let table = Table { mesh: Some(std::sync::Arc::new(mesh)), crs: Some(3006), ..Default::default() };
        let dir = std::env::temp_dir().join(format!("og-walk-{}", std::process::id()));
        let o = WalkOpts { start: Some([418305.0, 6472540.0]), ..Default::default() };
        let files = write_walk(&table, &dir, &GltfOpts { origin: Some([418300.0, 6472550.0, 0.0]), ..Default::default() }, &o).unwrap();
        assert_eq!(files.len(), 2);
        let html = std::fs::read_to_string(dir.join("index.html")).unwrap();
        // Where to begin is from the origin; the model is the file beside, in base64.
        assert!(html.contains("const EYE = 1.7, SPEED = 3, START = [5, -10];"));
        let glb = std::fs::read(dir.join("scene.glb")).unwrap();
        let data = base64::engine::general_purpose::STANDARD.encode(&glb);
        assert!(html.contains(&data) && glb.starts_with(b"glTF"));
        // Without a model there is nothing to walk in.
        assert!(write_walk(&Table::default(), &dir, &GltfOpts::default(), &o).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
