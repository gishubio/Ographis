//! Lighten 3D model, added in 0.96.3: fewer triangles, smaller textures and one texture for
//! many materials, for a model that is to be shown in realtime, such as in a browser, a game
//! engine or on a phone.

use super::geom_transform::number_param;
use super::*;
use crate::model::{err, fmt_count, fmt_real};
use std::sync::Arc;

const PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "most",
        label: "Triangles at most",
        kind: ParamKind::Text,
        default: "100000",
        hint: "In all, shared among the objects by how many they have. Empty: as few as Largest change allows",
    },
    ParamSpec {
        key: "error",
        label: "Largest change",
        kind: ParamKind::Text,
        default: "",
        hint: "How far in metres the surface may move, such as 0.5. Empty: as far as it must to reach the number of triangles",
    },
    ParamSpec {
        key: "keep_edges",
        label: "Keep the edges",
        kind: ParamKind::Choice(YES_NO),
        default: "true",
        hint: "The edges of each object and material stay where they are, so that the pieces still meet without gaps",
    },
    ParamSpec {
        key: "texture_most",
        label: "Largest texture",
        kind: ParamKind::Text,
        default: "2048",
        hint: "In pixels on the longer side; larger textures are made smaller. Empty: as they are",
    },
    ParamSpec {
        key: "atlas",
        label: "One texture for the materials",
        kind: ParamKind::Choice(YES_NO),
        default: "true",
        hint: "Plain colours and textures that do not repeat go into one texture, an atlas, so that the model is drawn in few calls. Textures that repeat, such as facades, and glass keep their own",
    },
];

fn run_lighten(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Model");
    let Some(mesh) = table.mesh.as_deref() else {
        return err("Nothing came in with a 3D model. Connect one, such as from Make buildings, Terrain to 3D model, a 3D input or 3D Tiles.");
    };
    let most = number_param(p, "most", "Triangles at most")?.map(|m| m.max(4.0) as usize);
    let error = number_param(p, "error", "The largest change")?.map(|e| e.max(0.0));
    if most.is_none() && error.is_none() {
        return err("Say how light the model is to be: the triangles at most, the largest change in metres, or both.");
    }
    let s = crate::lighten::Simplify { most, error, keep_edges: p.get("keep_edges").as_deref() != Some("false") };
    let (mut light, done) = crate::lighten::simplify(mesh, &s);
    ctx.notes.info(format!(
        "{} triangles of {} kept, {} %; the surface moved at most {} m",
        fmt_count(done.after as u64),
        fmt_count(done.before as u64),
        fmt_real(((done.after as f64 / done.before.max(1) as f64) * 1000.0).round() / 10.0),
        fmt_real((done.error * 100.0).round() / 100.0)
    ));
    if let Some(m) = most {
        if done.after > m {
            ctx.notes.warn(format!(
                "{} triangles are more than {}: objects of a few triangles each, such as boxes, are kept as they are{}.",
                fmt_count(done.after as u64),
                fmt_count(m as u64),
                if s.keep_edges { ", and the edges are kept" } else { "" }
            ));
        }
    }
    if let Some(t) = number_param(p, "texture_most", "The largest texture")? {
        let n = crate::lighten::shrink_textures(&mut light, t.max(16.0) as usize);
        if n > 0 {
            ctx.notes.info(format!("{} textures made smaller, to at most {} pixels", n, t.max(16.0) as usize));
        }
    }
    if p.get("atlas").as_deref() != Some("false") {
        let (atlased, a) = crate::lighten::atlas(&light, number_param(p, "texture_most", "The largest texture")?.unwrap_or(4096.0).max(64.0) as usize)?;
        if a.after < a.before {
            ctx.notes.info(format!(
                "{} materials put into one texture of {} × {} pixels; {} materials in all",
                a.before - a.after + 1,
                a.width,
                a.height,
                a.after
            ));
            light = atlased;
        }
        if a.repeating > 0 {
            ctx.notes.info(format!("{} textures repeat, such as facades, and keep their own.", a.repeating));
        }
    }
    let mut out = table.clone();
    out.mesh = Some(Arc::new(light));
    Ok(vec![("Model".into(), out)])
}

pub(super) const TYPES: &[NodeType] = &[NodeType {
    name: "ModelLightener",
    title: "Lighten 3D model",
    hidden: false,
    group: Group::Transformer,
    category: "3D models and views",
    desc: "Makes a 3D model lighter, to be shown in realtime in a browser, a game engine or on a phone: fewer triangles where they show little, by meshoptimizer, to a number of triangles or a largest change in metres, with the edges of each object kept; textures no larger than needed; and the plain colours and textures put together into one texture, an atlas, so that the model is drawn in few calls. Each object keeps its attributes. Write it as glTF with the compression of meshoptimizer, as 3D Tiles or as a walk-through.",
    inputs: &["Model"],
    outputs: &["Model"],
    ports: None,
    params: PARAMS,
    run: run_lighten,
    summary: |p| match (p.get("most").filter(|m| !m.trim().is_empty()), p.get("error").filter(|e| !e.trim().is_empty())) {
        (Some(m), _) => format!("at most {} triangles", m),
        (None, Some(e)) => format!("within {} m", e),
        _ => String::new(),
    },
}];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{Material, Mesh, Tri, NONE};

    #[test]
    fn a_model_is_lightened_and_keeps_its_objects() {
        let n = 60;
        let mut m = Mesh { materials: vec![Material::default()], objects: vec!["ground".into()], ..Default::default() };
        for y in 0..=n {
            for x in 0..=n {
                m.positions.push([x as f64, y as f64, 0.0]);
            }
        }
        let at = |x: usize, y: usize| (y * (n + 1) + x) as u32;
        for y in 0..n {
            for x in 0..n {
                for v in [[at(x, y), at(x + 1, y), at(x + 1, y + 1)], [at(x, y), at(x + 1, y + 1), at(x, y + 1)]] {
                    m.tris.push(Tri { v, n: [NONE; 3], t: [NONE; 3], material: 0, object: 0 });
                }
            }
        }
        let table = Table { mesh: Some(Arc::new(m)), features: vec![crate::model::Feature::new(vec![], None)], ..Default::default() };
        let (out, notes) = run_for_test("ModelLightener", &[("most", "500")], vec![("Model", table)]).unwrap();
        let light = out[0].1.mesh.as_deref().unwrap();
        assert!(light.tris.len() <= 500 && light.objects == vec!["ground".to_string()], "{:?}", notes);
        assert!(run_for_test("ModelLightener", &[("most", "")], vec![("Model", Table::default())]).is_err());
    }
}
