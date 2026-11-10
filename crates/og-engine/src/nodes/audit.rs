//! A look over all the tools: each is run with its own settings on a small mixed stream,
//! points, lines and areas with a few attributes, and those that give nothing without a word
//! in the log are listed. Run with `cargo test -p og-engine audit -- --ignored --nocapture`.

#[cfg(test)]
mod tests {
    use crate::model::{Feature, Field, FieldType, Table, Value};
    use crate::nodes::{run_for_test, Group, NODE_TYPES};
    use geo::{line_string, point, polygon, Geometry};

    fn mixed() -> Table {
        let mut t = Table {
            fields: vec![Field::new("name", FieldType::Text), Field::new("value", FieldType::Int), Field::new("speed", FieldType::Real)],
            crs: Some(3006),
            ..Default::default()
        };
        let a = |n: &str, v: i64, g: Geometry<f64>| Feature::new(vec![Value::Text(n.into()), Value::Int(v), Value::Real(50.0)], Some(g));
        for k in 0..4 {
            let x = 400_000.0 + k as f64 * 300.0;
            let y = 6_400_000.0;
            t.features.push(a("point", k, Geometry::Point(point!(x: x + 10.0, y: y + 10.0))));
            t.features.push(a("line", k, Geometry::LineString(line_string![(x: x, y: y), (x: x + 300.0, y: y), (x: x + 300.0, y: y + 300.0)])));
            t.features.push(a(
                "area",
                k,
                Geometry::Polygon(polygon![(x: x, y: y), (x: x + 200.0, y: y), (x: x + 200.0, y: y + 200.0), (x: x, y: y + 200.0), (x: x, y: y)]),
            ));
        }
        t
    }

    #[test]
    fn audit_no_tool_gives_nothing_without_a_word() {
        let mut silent = Vec::new();
        for t in NODE_TYPES.iter().filter(|t| !t.hidden && t.group != Group::Reader && t.group != Group::Writer) {
            if matches!(t.category, "Web" | "Database") {
                continue;
            }
            let given: Vec<(&'static str, Table)> = t.inputs.iter().map(|p| (*p, mixed())).collect();
            let started = std::time::Instant::now();
            let r = std::panic::catch_unwind(|| run_for_test(t.name, &[], given));
            let took = started.elapsed().as_secs_f64();
            match r {
                Err(_) => println!("PANIC  {}", t.name),
                Ok(Err(e)) => println!("error  {:<28} {}", t.name, e.0),
                Ok(Ok((outs, notes))) => {
                    let went: usize = outs.iter().map(|(_, t)| t.count()).sum();
                    let other = outs.iter().any(|(_, t)| t.image.is_some() || t.grid.is_some() || t.mesh.is_some());
                    if went == 0 && !other && notes.warnings.is_empty() && !t.outputs.is_empty() {
                        silent.push(t.name);
                        println!("SILENT {:<28} info: {}", t.name, notes.info.join(" | "));
                    } else {
                        println!(
                            "ok     {:<28} {} out{}",
                            t.name,
                            went,
                            if notes.warnings.is_empty() { String::new() } else { format!(" · warns: {}", notes.warnings.join(" | ")) }
                        );
                    }
                }
            }
            if took > 5.0 {
                println!("SLOW   {} {:.1} s", t.name, took);
            }
        }
        println!("\n{} silent: {:?}", silent.len(), silent);
        // Inside a tool, Tool input gives nothing when the model runs on its own, and says so.
        silent.retain(|n| *n != "ToolInput");
        assert!(silent.is_empty(), "These give nothing and say nothing about why: {:?}", silent);
    }
}
