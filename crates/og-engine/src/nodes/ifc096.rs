//! Check against IDS: the objects of an IFC model checked against an Information Delivery
//! Specification.

use super::*;
use crate::model::{err, Field, FieldType, GbError, Result, Value};

const IDS_PARAMS: &[ParamSpec] = &[ParamSpec {
    key: "path",
    label: "IDS file",
    kind: ParamKind::FileOpen,
    default: "",
    hint: "The requirements as buildingSMART's Information Delivery Specification (.ids), such as a client's demands on the model",
}];

fn run_ids(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let file = ctx.resolve(&p.need("path", "Choose the IDS file with the requirements.")?);
    let text = std::fs::read_to_string(&file).map_err(|e| GbError(format!("Cannot read {}: {}", file.display(), e)))?;
    let specs = crate::ids::read(&text)?;
    if table.field_index("IfcClass").is_none() && !table.is_empty() {
        return err("Connect objects read from an IFC file: they are checked by their class, attributes and property sets, which this stream does not have.");
    }
    let checked = crate::ids::check(&specs, table);
    let (mut passed, mut failed) = (table.like(), table.like());
    for t in [&mut passed, &mut failed] {
        t.fields.push(Field::new("ids_specs", FieldType::Int));
        t.fields.push(Field::new("ids_failed", FieldType::Text));
    }
    for (row, f) in table.features.iter().enumerate() {
        let mut g = f.clone();
        g.attrs.push(Value::Int(checked.applied[row] as i64));
        g.attrs.push(crate::ids::joined(&checked.failures[row]));
        if checked.failures[row].is_empty() {
            passed.features.push(g);
        } else {
            failed.features.push(g);
        }
    }
    for (name, n, bad, min) in &checked.per_spec {
        if *n == 0 && *min > 0 {
            ctx.notes.warn(format!("{}: the model must have such objects, and has none.", name));
        } else if *n == 0 {
            ctx.notes.info(format!("{}: applies to no object", name));
        } else {
            ctx.notes.info(format!("{}: {} of {} objects meet it", name, fmt_count((n - bad) as u64), fmt_count(*n as u64)));
        }
    }
    for what in &checked.unchecked {
        ctx.notes.warn(format!("{} cannot be checked from what Ographis reads of IFC, and was left out.", what));
    }
    let applied = checked.applied.iter().filter(|a| **a > 0).count();
    if applied == 0 && !table.is_empty() {
        ctx.notes.warn("No specification of the IDS applies to any of the objects that came in, so all of them passed.");
    }
    ctx.notes.info(format!("{} objects checked against {} specifications, {} fail", fmt_count(applied as u64), specs.len(), fmt_count(failed.len() as u64)));
    Ok(vec![("Passed".into(), passed), ("Failed".into(), failed)])
}

pub(super) const TYPES: &[NodeType] = &[NodeType {
    name: "IdsChecker",
    title: "Check against IDS",
    hidden: false,
    group: Group::Transformer,
    category: "3D models and views",
    desc: "Checks the objects of an IFC model against an Information Delivery Specification (IDS 1.0): for each specification, the objects it applies to by class, attribute, property, material or storey, and what they must have, may have or must not have, with values as they are or as a list, a pattern or a range. Objects that meet every specification that applies go to Passed, the others to Failed with what they lack in ids_failed. The log tells how many meet each specification. Classifications are not read from IFC, so they cannot be checked, and the log says so.",
    inputs: &["Input"],
    outputs: &["Passed", "Failed"],
    ports: None,
    params: IDS_PARAMS,
    run: run_ids,
    summary: |p| p.get("path").map(|f| f.rsplit(['/', '\\']).next().unwrap_or("").to_string()).unwrap_or_else(|| "no IDS file".into()),
}];

#[cfg(test)]
mod tests {
    use super::*;

    const IDS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ids:ids xmlns:ids="http://standards.buildingsmart.org/IDS" xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <ids:info><ids:title>Krav</ids:title></ids:info>
  <ids:specifications>
    <ids:specification name="Walls have a fire rating" ifcVersion="IFC4">
      <ids:applicability minOccurs="1">
        <ids:entity><ids:name><ids:simpleValue>IFCWALL</ids:simpleValue></ids:name></ids:entity>
      </ids:applicability>
      <ids:requirements>
        <ids:property cardinality="required">
          <ids:propertySet><ids:simpleValue>Pset_WallCommon</ids:simpleValue></ids:propertySet>
          <ids:baseName><ids:simpleValue>FireRating</ids:simpleValue></ids:baseName>
          <ids:value><xs:restriction base="xs:string"><xs:enumeration value="EI30"/><xs:enumeration value="EI60"/></xs:restriction></ids:value>
        </ids:property>
        <ids:attribute cardinality="required"><ids:name><ids:simpleValue>Name</ids:simpleValue></ids:name></ids:attribute>
      </ids:requirements>
    </ids:specification>
    <ids:specification name="Doors are not of wood">
      <ids:applicability><ids:entity><ids:name><ids:simpleValue>IFCDOOR</ids:simpleValue></ids:name></ids:entity></ids:applicability>
      <ids:requirements>
        <ids:material cardinality="prohibited"><ids:value><xs:restriction><xs:pattern value=".*[Ww]ood.*"/></xs:restriction></ids:value></ids:material>
      </ids:requirements>
    </ids:specification>
  </ids:specifications>
</ids:ids>"#;

    #[test]
    fn objects_are_checked_against_an_ids() {
        let dir = std::env::temp_dir().join(format!("og-ids-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("krav.ids"), IDS).unwrap();
        let mut t = crate::model::Table::default();
        for n in ["GlobalId", "IfcClass", "Name", "material", "Pset_WallCommon.FireRating"] {
            t.fields.push(Field::new(n, FieldType::Text));
        }
        let row = |v: [&str; 5]| crate::model::Feature {
            attrs: v.iter().map(|s| if s.is_empty() { Value::Null } else { Value::Text(s.to_string()) }).collect(),
            geom: None,
            z: None,
        };
        t.features = vec![
            row(["a", "IfcWall", "Wall 1", "Concrete", "EI60"]),
            row(["b", "IfcWall", "Wall 2", "Concrete", "REI90"]),
            row(["c", "IfcWall", "", "Brick", ""]),
            row(["d", "IfcDoor", "Door", "Oak wood", ""]),
            row(["e", "IfcWindow", "Window", "Glass", ""]),
        ];
        let p = format!("{}", dir.join("krav.ids").display());
        let (out, notes) = run_for_test("IdsChecker", &[("path", p.as_str())], vec![("Input", t)]).unwrap();
        let get = |port: &str| out.iter().find(|o| o.0 == port).unwrap().1.clone();
        let (passed, failed) = (get("Passed"), get("Failed"));
        let ids: Vec<String> = failed.features.iter().map(|f| f.attrs[0].to_text()).collect();
        assert_eq!(ids, ["b", "c", "d"]);
        let why = |k: usize| failed.features[k].attrs[6].to_text();
        assert!(why(0).contains("FireRating"), "{}", why(0));
        assert!(why(1).contains("FireRating") && why(1).contains("Name"), "{}", why(1));
        assert!(why(2).contains("must not have a material"), "{}", why(2));
        assert_eq!(passed.len(), 2);
        assert_eq!(passed.features[1].attrs[5], Value::Int(0), "the window: no specification applies");
        assert!(notes.info.iter().any(|n| n.contains("1 of 3 objects meet it")), "{:?}", notes.info);
        let _ = std::fs::remove_dir_all(dir);
    }
}
