//! Draw chart: a bar chart of the features, as a picture.

use super::*;
use crate::chart::{self, Chart, Labels, Orientation};
use crate::model::{err, fmt_count, Result, Value};

pub(super) const SORTS: &[(&str, &str)] = &[("down", "Largest first"), ("up", "Smallest first"), ("name", "Alphabetical"), ("none", "As they come")];
pub(super) const COMBINE: &[(&str, &str)] = &[("sum", "Sum"), ("mean", "Mean"), ("max", "Largest"), ("min", "Smallest")];
pub(super) const ORIENTATIONS: &[(&str, &str)] = &[("auto", "Choose by the data"), ("bars", "Bars across"), ("columns", "Columns")];
pub(super) const ANGLES: &[(&str, &str)] = &[("0", "Level"), ("45", "Turned 45°"), ("90", "Upright, 90°")];
pub(super) const LABELS: &[(&str, &str)] = &[("all", "On every bar"), ("ends", "Largest and smallest"), ("none", "None")];
pub(super) const FORMATS: &[(&str, &str)] = &[("sv", "1 234,5"), ("en", "1,234.5")];
const YES_NO: &[(&str, &str)] = &[("no", "No"), ("yes", "Yes")];
pub(super) const BACKGROUNDS: &[(&str, &str)] = &[("paper", "Off-white"), ("white", "White"), ("none", "See-through")];
pub(super) const COLOURS: &[(&str, &str)] = &[
    ("blue", "Blue"),
    ("orange", "Orange"),
    ("aqua", "Aqua"),
    ("yellow", "Yellow"),
    ("magenta", "Magenta"),
    ("green", "Green"),
    ("violet", "Violet"),
    ("red", "Red"),
    ("grey", "Grey"),
];

pub(super) const CHART_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "category",
        label: "Names",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "The attribute that names each bar, such as the name of the municipality",
    },
    ParamSpec {
        key: "value",
        label: "Values",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "The number each bar shows, such as the area. Empty counts the features with each name",
    },
    ParamSpec {
        key: "combine",
        label: "Features with the same name",
        kind: ParamKind::Choice(COMBINE),
        default: "sum",
        hint: "How their values are put together into one bar",
    },
    ParamSpec { key: "sort", label: "Order", kind: ParamKind::Choice(SORTS), default: "down", hint: "" },
    ParamSpec {
        key: "limit",
        label: "Show at most",
        kind: ParamKind::Text,
        default: "",
        hint: "The first bars in that order, such as 10 for a top ten. Empty shows them all",
    },
    ParamSpec { key: "title", label: "Title", kind: ParamKind::Text, default: "", hint: "Say what the chart shows, in a few words" },
    ParamSpec { key: "subtitle", label: "Explanation", kind: ParamKind::Text, default: "", hint: "A line under the title: what is measured, where and when" },
    ParamSpec { key: "note", label: "Note at the foot", kind: ParamKind::Text, default: "", hint: "Such as where the data comes from" },
    ParamSpec { key: "unit", label: "Unit", kind: ParamKind::Text, default: "", hint: "Written after the numbers, such as km²" },
    ParamSpec { key: "decimals", label: "Decimals", kind: ParamKind::Text, default: "0", hint: "" },
    ParamSpec { key: "format", label: "Numbers as", kind: ParamKind::Choice(FORMATS), default: "sv", hint: "" },
    ParamSpec { key: "labels", label: "Numbers on the bars", kind: ParamKind::Choice(LABELS), default: "all", hint: "" },
    ParamSpec { key: "mean", label: "Line at the mean", kind: ParamKind::Choice(YES_NO), default: "no", hint: "" },
    ParamSpec {
        key: "orientation",
        label: "Bars",
        kind: ParamKind::Choice(ORIENTATIONS),
        default: "auto",
        hint: "Many bars or long names are drawn across, a few short ones as columns",
    },
    ParamSpec {
        key: "label_angle",
        label: "Names under columns",
        kind: ParamKind::Choice(ANGLES),
        default: "0",
        hint: "Turned, many names fit under narrow columns, such as 49 municipalities",
    },
    ParamSpec { key: "colour", label: "Colour", kind: ParamKind::Choice(COLOURS), default: "blue", hint: "" },
    ParamSpec {
        key: "highlight",
        label: "Bars to point out",
        kind: ParamKind::Values("Input", "category"),
        default: "",
        hint: "Names whose bars get the second colour, one per line",
    },
    ParamSpec { key: "accent", label: "Second colour", kind: ParamKind::Choice(COLOURS), default: "orange", hint: "" },
    ParamSpec { key: "width", label: "Width in pixels", kind: ParamKind::Text, default: "1200", hint: "The text grows with the width" },
    ParamSpec { key: "height", label: "Height in pixels", kind: ParamKind::Text, default: "", hint: "Empty makes it fit the bars" },
    ParamSpec { key: "background", label: "Background", kind: ParamKind::Choice(BACKGROUNDS), default: "paper", hint: "" },
    ParamSpec {
        key: "html",
        label: "Also as a web page",
        kind: ParamKind::FileSave,
        default: "",
        hint:
            "An .html file with the chart that shows each value under the pointer and can be sorted, to open in any web browser or put on a site. Empty: none",
    },
];

/// A key that puts names in Swedish order: å, ä and ö after z, and case after letter.
pub fn alphabetical(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'å' => "{".to_string(),
            'ä' | 'æ' => "|".to_string(),
            'ö' | 'ø' => "}".to_string(),
            'é' | 'è' | 'ê' => "e".to_string(),
            'ü' => "y".to_string(),
            'á' | 'à' => "a".to_string(),
            c => c.to_string(),
        })
        .collect()
}

pub(super) fn run_chart(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let cat = p.need("category", "Choose the attribute that names the bars.")?;
    let ci = table.need_field(&cat)?;
    let vi = match p.get("value").filter(|v| !v.is_empty()) {
        Some(v) => Some(table.need_field(&v)?),
        None => None,
    };
    // One bar per name, in the order the names first come.
    let mut names: Vec<String> = Vec::new();
    let mut values: Vec<Vec<f64>> = Vec::new();
    let mut index = std::collections::HashMap::new();
    let mut skipped = 0usize;
    for f in &table.features {
        let name = f.attrs[ci].to_text();
        if name.trim().is_empty() {
            skipped += 1;
            continue;
        }
        let v = match vi {
            None => 1.0,
            Some(i) => match &f.attrs[i] {
                Value::Int(x) => *x as f64,
                Value::Real(x) if x.is_finite() => *x,
                Value::Text(t) => match crate::model::parse_number(t) {
                    Some(x) => x,
                    None => {
                        skipped += 1;
                        continue;
                    }
                },
                _ => {
                    skipped += 1;
                    continue;
                }
            },
        };
        let k = *index.entry(name.clone()).or_insert_with(|| {
            names.push(name.clone());
            values.push(Vec::new());
            names.len() - 1
        });
        values[k].push(v);
    }
    if names.is_empty() {
        return err("There is nothing to draw: no feature has both a name and a number.");
    }
    if skipped > 0 {
        ctx.notes.info(format!(
            "{} without a name or a number left out",
            if skipped == 1 { "1 feature".to_string() } else { format!("{} features", fmt_count(skipped as u64)) }
        ));
    }
    let combine = p.get("combine").unwrap_or_else(|| "sum".into());
    let mut bars: Vec<(String, f64)> = names
        .into_iter()
        .zip(values)
        .map(|(n, v)| {
            let x = if vi.is_none() {
                v.len() as f64
            } else {
                match combine.as_str() {
                    "mean" => v.iter().sum::<f64>() / v.len() as f64,
                    "max" => v.iter().copied().fold(f64::MIN, f64::max),
                    "min" => v.iter().copied().fold(f64::MAX, f64::min),
                    _ => v.iter().sum(),
                }
            };
            (n, x)
        })
        .collect();
    match p.get("sort").as_deref().unwrap_or("down") {
        "up" => bars.sort_by(|a, b| a.1.total_cmp(&b.1)),
        "name" => bars.sort_by_cached_key(|b| (alphabetical(&b.0), b.0.clone())),
        "none" => {}
        _ => bars.sort_by(|a, b| b.1.total_cmp(&a.1)),
    }
    if let Some(n) = number_param(p, "limit", "Show at most")? {
        if n < 1.0 {
            return err("Show at most must be 1 or more, or empty for all.");
        }
        bars.truncate(n as usize);
    }
    let width = number_param(p, "width", "The width")?.unwrap_or(1200.0);
    if !(200.0..=8000.0).contains(&width) {
        return err("The width must be from 200 to 8000 pixels.");
    }
    let height = number_param(p, "height", "The height")?;
    if let Some(h) = height {
        if !(150.0..=20000.0).contains(&h) {
            return err("The height must be from 150 to 20000 pixels, or empty to fit the bars.");
        }
    }
    let decimals = number_param(p, "decimals", "The decimals")?.unwrap_or(0.0).clamp(0.0, 6.0) as usize;
    let orientation = match p.get("orientation").as_deref() {
        Some("bars") => Orientation::Bars,
        Some("columns") => Orientation::Columns,
        // Columns for a few bars with short names, read from left to right; else bars across.
        // Turned names are for columns.
        _ => {
            let turned = p.get("label_angle").is_some_and(|a| a != "0");
            if turned || (bars.len() <= 12 && bars.iter().all(|b| b.0.chars().count() <= 10)) {
                Orientation::Columns
            } else {
                Orientation::Bars
            }
        }
    };
    let c = Chart {
        highlight: p.list_exact("highlight"),
        orientation,
        title: p.exact("title").trim().to_string(),
        subtitle: p.exact("subtitle").trim().to_string(),
        note: p.exact("note").trim().to_string(),
        unit: p.exact("unit").trim().to_string(),
        decimals,
        swedish: p.get("format").as_deref() != Some("en"),
        labels: match p.get("labels").as_deref() {
            Some("none") => Labels::None,
            Some("ends") => Labels::Ends,
            _ => Labels::All,
        },
        mean_line: p.get("mean").as_deref() == Some("yes"),
        colour: chart::colour(&p.get("colour").unwrap_or_default()),
        accent: chart::colour(&p.get("accent").unwrap_or_else(|| "orange".into())),
        width: width as u32,
        height: height.map(|h| h as u32),
        label_angle: p.get("label_angle").and_then(|a| a.parse::<f32>().ok()).unwrap_or(0.0).clamp(0.0, 90.0),
        background: match p.get("background").as_deref() {
            Some("white") => Some([255, 255, 255]),
            Some("none") => None,
            _ => Some(chart::PAPER),
        },
        bars,
    };
    let n = c.bars.len();
    if let Some(path) = p.get("html").filter(|h| !h.trim().is_empty()) {
        let mut path = ctx.resolve(&path);
        if path.extension().is_none() {
            path.set_extension("html");
        }
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| crate::model::GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
        }
        std::fs::write(&path, chart::html(&c)).map_err(|e| crate::model::GbError(format!("Cannot write {}: {}", path.display(), e)))?;
        ctx.notes.info(format!("the web page written to {}", path.display()));
        ctx.files.push(path);
    }
    let image = chart::draw(&c)?;
    ctx.notes.info(format!(
        "{} · {} × {} pixels",
        if n == 1 { "1 bar".to_string() } else { format!("{} bars", fmt_count(n as u64)) },
        image.width,
        image.height
    ));
    // A chart lies nowhere: it has pixel coordinates and no coordinate system.
    Ok(vec![("Chart".into(), image.into_table(None))])
}

/// The tools of this module, for the registry in mod.rs.
pub(super) const TYPES: &[NodeType] = &[
    NodeType {
        name: "ChartDrawer",
        title: "Draw chart",
        hidden: false,
        group: Group::Transformer,
        category: "Charts",
        desc: "Draws a bar chart of the features: a bar for each name, as long as its value, such as the area of each municipality. Sorted by size or by name, with a title, an explanation and a note. The chart comes out as a picture to write to a .png or a .pdf, and can also be written as an interactive web page, where each value shows under the pointer and the bars can be sorted.",
        inputs: &["Input"],
        outputs: &["Chart"],
        ports: None,
        params: CHART_PARAMS,
        run: run_chart,
        summary: |p| {
            let what = p.get("value").filter(|v| !v.is_empty()).unwrap_or_else(|| "count".into());
            let sort = match p.get("sort").as_deref() {
                Some("up") => "smallest first",
                Some("name") => "by name",
                Some("none") => "as they come",
                _ => "largest first",
            };
            match p.get("category").filter(|c| !c.is_empty()) {
                Some(c) => format!("{} per {} · {}", what, c, sort),
                None => "no names chosen".to_string(),
            }
        },
    },
];

#[cfg(test)]
mod tests {
    use super::alphabetical;

    #[test]
    fn swedish_letters_come_after_z() {
        let mut v = vec!["Öckerö", "Åmål", "Ale", "Ärla", "Zinkgruvan", "borås"];
        v.sort_by_key(|s| alphabetical(s));
        assert_eq!(v, ["Ale", "borås", "Zinkgruvan", "Åmål", "Ärla", "Öckerö"]);
    }
}
