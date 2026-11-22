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

const KINDS: &[(&str, &str)] = &[
    ("bars", "Bars"),
    ("lines", "Lines"),
    ("histogram", "Histogram: how the values are spread"),
    ("scatter", "Scatter: one number against another"),
    ("box", "Box plot: the spread of each group"),
    ("stacked", "Stacked bars: the parts of each"),
    ("grouped", "Grouped bars: the parts side by side"),
    ("pyramid", "Population pyramid"),
];

pub(super) const CHART_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "kind",
        label: "Chart",
        kind: ParamKind::Choice(KINDS),
        default: "bars",
        hint: "Lines show how a value changes, such as the inhabitants year by year",
    },
    ParamSpec {
        key: "x",
        label: "Along",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "For lines: the attribute along the bottom, a number such as a year, or a date",
    },
    ParamSpec {
        key: "series",
        label: "A line for each",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "For lines: an attribute whose values each get a line of their own, such as the municipality. Empty draws one line",
    },
    ParamSpec {
        key: "from_zero",
        label: "The scale starts at 0",
        kind: ParamKind::Choice(YES_NO),
        default: "no",
        hint: "For lines. Else it starts near the smallest value",
    },
    ParamSpec { key: "markers", label: "A dot at each point", kind: ParamKind::Choice(YES_NO), default: "no", hint: "For lines" },
    ParamSpec {
        key: "facet",
        label: "One chart for each",
        kind: ParamKind::Attribute("Input"),
        default: "",
        hint: "Small multiples: a small chart for each value, such as each municipality, side by side on one scale. Empty draws one chart",
    },
    ParamSpec {
        key: "free_scales",
        label: "Each small chart its own scale",
        kind: ParamKind::Choice(YES_NO),
        default: "no",
        hint: "One shared scale lets them be compared; scales of their own show the shape of each",
    },
    ParamSpec {
        key: "bins",
        label: "Bins",
        kind: ParamKind::Text,
        default: "",
        hint: "For a histogram: about how many. Empty chooses by the number of values",
    },
    ParamSpec {
        key: "trend",
        label: "Trend line",
        kind: ParamKind::Choice(YES_NO),
        default: "yes",
        hint: "For a scatter plot: a straight line through the points, with R², how much of the spread it explains",
    },
    ParamSpec {
        key: "percent",
        label: "As shares of 100 %",
        kind: ParamKind::Choice(YES_NO),
        default: "no",
        hint: "For stacked bars: every bar as tall, the parts as shares",
    },
    ParamSpec {
        key: "x_title",
        label: "Along the bottom",
        kind: ParamKind::Text,
        default: "",
        hint: "For a histogram or a scatter plot: what the numbers along the bottom are",
    },
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

/// A value along the bottom of a line chart: a number, or a date as days since 1970.
fn x_value(v: &Value) -> Option<(f64, bool)> {
    match v {
        Value::Int(x) => Some((*x as f64, false)),
        Value::Real(x) if x.is_finite() => Some((*x, false)),
        Value::Text(t) => {
            let t = t.trim();
            if let Some(n) = crate::model::parse_number(t) {
                return Some((n, false));
            }
            let day = |d: chrono::NaiveDate| (d - chrono::NaiveDate::from_ymd_opt(1970, 1, 1).expect("a date")).num_days() as f64;
            let first10: String = t.chars().take(10).collect();
            chrono::NaiveDate::parse_from_str(&first10, "%Y-%m-%d")
                .ok()
                .or_else(|| chrono::NaiveDate::parse_from_str(&format!("{}-01", t), "%Y-%m-%d").ok())
                .map(|d| (day(d), true))
        }
        _ => None,
    }
}

fn run_lines(p: &P, table: &Table, ctx: &mut NodeCtx) -> Result<Outputs> {
    let xi = table.need_field(&p.need("x", "Choose the attribute along the bottom, such as the year.")?)?;
    let yi = table.need_field(&p.need("value", "Choose the values the line goes through.")?)?;
    let si = match p.get("series").filter(|v| !v.is_empty()) {
        Some(v) => Some(table.need_field(&v)?),
        None => None,
    };
    let combine = p.get("combine").unwrap_or_else(|| "sum".into());
    // The points of each series, with points at the same x put together.
    let mut names: Vec<String> = Vec::new();
    let mut points: Vec<std::collections::BTreeMap<i64, (f64, Vec<f64>)>> = Vec::new();
    let (mut dates, mut skipped) = (false, 0usize);
    for f in &table.features {
        let (Some((x, d)), Some(y)) = (x_value(&f.attrs[xi]), f.attrs[yi].as_f64().or_else(|| crate::model::parse_number(&f.attrs[yi].to_text()))) else {
            skipped += 1;
            continue;
        };
        dates |= d;
        let name = si.map(|i| f.attrs[i].to_text()).unwrap_or_default();
        let k = match names.iter().position(|n| *n == name) {
            Some(k) => k,
            None => {
                names.push(name);
                points.push(Default::default());
                names.len() - 1
            }
        };
        points[k].entry((x * 1e6).round() as i64).or_insert((x, Vec::new())).1.push(y);
    }
    if names.is_empty() {
        return err("There is nothing to draw: no feature has both a value along the bottom and a number.");
    }
    if skipped > 0 {
        ctx.notes.info(format!("{} features without a value along the bottom or a number left out", fmt_count(skipped as u64)));
    }
    let mut series: Vec<(String, Vec<(f64, f64)>)> = names
        .into_iter()
        .zip(points)
        .map(|(n, pts)| {
            let pts = pts
                .into_values()
                .map(|(x, ys)| {
                    let y = match combine.as_str() {
                        "mean" => ys.iter().sum::<f64>() / ys.len() as f64,
                        "max" => ys.iter().copied().fold(f64::MIN, f64::max),
                        "min" => ys.iter().copied().fold(f64::MAX, f64::min),
                        _ => ys.iter().sum(),
                    };
                    (x, y)
                })
                .collect();
            (n, pts)
        })
        .collect();
    if series.len() > 20 {
        ctx.notes.warn(format!("{} lines are hard to tell apart. The 20 with the largest last values are drawn.", series.len()));
        series.sort_by(|a, b| b.1.last().map_or(0.0, |p| p.1).total_cmp(&a.1.last().map_or(0.0, |p| p.1)));
        series.truncate(20);
    }
    let width = number_param(p, "width", "The width")?.unwrap_or(1200.0);
    if !(200.0..=8000.0).contains(&width) {
        return err("The width must be from 200 to 8000 pixels.");
    }
    let c = chart::LineChart {
        series,
        dates,
        title: p.exact("title").trim().to_string(),
        subtitle: p.exact("subtitle").trim().to_string(),
        note: p.exact("note").trim().to_string(),
        unit: p.exact("unit").trim().to_string(),
        decimals: number_param(p, "decimals", "The decimals")?.unwrap_or(0.0).clamp(0.0, 6.0) as usize,
        swedish: p.get("format").as_deref() != Some("en"),
        from_zero: p.get("from_zero").as_deref() == Some("yes"),
        markers: p.get("markers").as_deref() == Some("yes"),
        colour: chart::colour(&p.get("colour").unwrap_or_default()),
        width: width as u32,
        height: number_param(p, "height", "The height")?.map(|h| h.clamp(150.0, 20000.0) as u32),
        background: match p.get("background").as_deref() {
            Some("white") => Some([255, 255, 255]),
            Some("none") => None,
            _ => Some(chart::PAPER),
        },
    };
    if p.get("html").is_some_and(|h| !h.trim().is_empty()) {
        ctx.notes.warn("The web page is made for bar charts only, so none was written for the lines.");
    }
    let image = chart::draw_lines(&c)?;
    ctx.notes.info(format!("{} lines · {} × {} pixels", c.series.len(), image.width, image.height));
    Ok(vec![("Chart".into(), image.into_table(None))])
}

pub(super) fn run_chart(p: &P, inp: &Inputs, ctx: &mut NodeCtx) -> Result<Outputs> {
    let table = input(inp, "Input");
    let kind = p.get("kind").unwrap_or_else(|| "bars".into());
    let facet = p.get("facet").is_some_and(|f| !f.is_empty());
    if facet || !matches!(kind.as_str(), "bars" | "lines") {
        return run_plot(p, &kind, table, ctx);
    }
    if kind == "lines" {
        return run_lines(p, table, ctx);
    }
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

/// A number of an attribute, also when written as text.
fn number_of(v: &Value) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite()).or_else(|| crate::model::parse_number(&v.to_text()))
}

/// Puts values together as the setting Features with the same name says.
fn combined(v: &[f64], how: &str) -> f64 {
    match how {
        "mean" => v.iter().sum::<f64>() / v.len().max(1) as f64,
        "max" => v.iter().copied().fold(f64::MIN, f64::max),
        "min" => v.iter().copied().fold(f64::MAX, f64::min),
        _ => v.iter().sum(),
    }
}

/// The names in the order the features first have them.
fn first_seen(table: &Table, rows: &[usize], i: usize) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    rows.iter().map(|r| table.features[*r].attrs[i].to_text()).filter(|n| seen.insert(n.clone())).collect()
}

/// Histograms, scatter plots, box plots, stacked bars, pyramids, and small multiples of any.
fn run_plot(p: &P, kind: &str, table: &Table, ctx: &mut NodeCtx) -> Result<Outputs> {
    use crate::plot::{Panel, Plot};
    let field = |key: &str| -> Result<Option<usize>> {
        match p.get(key).filter(|v| !v.is_empty()) {
            Some(v) => Ok(Some(table.need_field(&v)?)),
            None => Ok(None),
        }
    };
    let need = |key: &str, what: &str| -> Result<usize> { table.need_field(&p.need(key, what)?) };
    let combine = p.get("combine").unwrap_or_else(|| "sum".into());
    let sort = p.get("sort").unwrap_or_else(|| "down".into());
    let all: Vec<usize> = (0..table.len()).collect();
    // The series: their names, the same in every small chart.
    let si = field("series")?;
    let mut series: Vec<String> = si.map(|i| first_seen(table, &all, i)).unwrap_or_default();
    if matches!(kind, "stacked" | "grouped" | "pyramid" | "lines") {
        series.sort_by_cached_key(|s| (alphabetical(s), s.clone()));
    }
    let series_of = |r: usize| si.map_or(0, |i| series.iter().position(|s| *s == table.features[r].attrs[i].to_text()).unwrap_or(0));
    // Categories in one order for every small chart.
    let order_names = |ci: usize, vi: Option<usize>| -> Vec<String> {
        let mut names = first_seen(table, &all, ci);
        let total = |n: &String| -> f64 {
            all.iter()
                .filter(|r| table.features[**r].attrs[ci].to_text() == *n)
                .map(|r| vi.and_then(|v| number_of(&table.features[*r].attrs[v])).unwrap_or(1.0))
                .sum()
        };
        match sort.as_str() {
            "name" => names.sort_by_cached_key(|b| (alphabetical(b), b.clone())),
            "none" => {}
            "up" => names.sort_by(|a, b| total(a).total_cmp(&total(b))),
            _ => names.sort_by(|a, b| total(b).total_cmp(&total(a))),
        }
        names
    };
    let mut left_out = 0usize;
    let build = |rows: &[usize], left_out: &mut usize| -> Result<Panel> {
        Ok(match kind {
            "histogram" => {
                let vi = need("value", "Choose the values to count into bins.")?;
                let v: Vec<f64> = rows.iter().filter_map(|r| number_of(&table.features[*r].attrs[vi])).collect();
                *left_out += rows.len() - v.len();
                Panel::Histogram(v)
            }
            "scatter" => {
                let xi = need("x", "Choose the number along the bottom.")?;
                let yi = need("value", "Choose the number up the side.")?;
                let pts: Vec<(f64, f64, usize)> = rows
                    .iter()
                    .filter_map(|r| Some((number_of(&table.features[*r].attrs[xi])?, number_of(&table.features[*r].attrs[yi])?, series_of(*r))))
                    .collect();
                *left_out += rows.len() - pts.len();
                Panel::Scatter(pts)
            }
            "box" => {
                let vi = need("value", "Choose the values whose spread is drawn.")?;
                let groups = match field("category")? {
                    Some(ci) => order_names(ci, None).into_iter().map(|g| (g, Vec::new())).collect::<Vec<_>>(),
                    None => vec![(String::new(), Vec::new())],
                };
                let mut groups = groups;
                let ci = field("category")?;
                for r in rows {
                    let f = &table.features[*r];
                    let Some(v) = number_of(&f.attrs[vi]) else {
                        *left_out += 1;
                        continue;
                    };
                    let g = ci.map_or(0, |ci| groups.iter().position(|g| g.0 == f.attrs[ci].to_text()).unwrap_or(0));
                    groups[g].1.push(v);
                }
                Panel::Boxes(groups)
            }
            "stacked" | "grouped" | "pyramid" | "bars" => {
                let ci = need(
                    "category",
                    if kind == "pyramid" {
                        "Choose the attribute with the groups, such as the age group."
                    } else {
                        "Choose the attribute that names the bars."
                    },
                )?;
                let vi = field("value")?;
                let mut cats = order_names(ci, vi);
                if kind == "pyramid" && sort != "none" {
                    // The groups in their own order, the first at the foot, such as the ages.
                    let start = |c: &str| crate::model::parse_number(c.split(['-', '–', ' ', '+']).next().unwrap_or("")).unwrap_or(f64::MAX);
                    cats.sort_by(|a, b| start(a).total_cmp(&start(b)).then_with(|| alphabetical(a).cmp(&alphabetical(b))));
                }
                let ns = if kind == "bars" { 1 } else { series.len().max(1) };
                let mut cells: Vec<Vec<Vec<f64>>> = vec![vec![Vec::new(); cats.len()]; ns];
                for r in rows {
                    let f = &table.features[*r];
                    let v = match vi {
                        Some(vi) => match number_of(&f.attrs[vi]) {
                            Some(v) => v,
                            None => {
                                *left_out += 1;
                                continue;
                            }
                        },
                        None => 1.0,
                    };
                    let Some(c) = cats.iter().position(|c| *c == f.attrs[ci].to_text()) else { continue };
                    let s = if kind == "bars" { 0 } else { series_of(*r) };
                    cells[s][c].push(v);
                }
                let how = if vi.is_none() { "sum" } else { combine.as_str() };
                let values: Vec<Vec<f64>> = cells.iter().map(|s| s.iter().map(|c| if c.is_empty() { 0.0 } else { combined(c, how) }).collect()).collect();
                match kind {
                    "bars" => Panel::Bars(cats.into_iter().zip(values[0].iter().copied()).collect()),
                    "pyramid" => Panel::Pyramid { groups: cats, left: values[0].clone(), right: values.get(1).cloned().unwrap_or_default() },
                    _ => Panel::Stacked { categories: cats, values },
                }
            }
            _ => {
                // Lines.
                let xi = need("x", "Choose the attribute along the bottom, such as the year.")?;
                let yi = need("value", "Choose the values the line goes through.")?;
                let mut pts: Vec<std::collections::BTreeMap<i64, (f64, Vec<f64>)>> = vec![Default::default(); series.len().max(1)];
                for r in rows {
                    let f = &table.features[*r];
                    let (Some((x, _)), Some(y)) = (x_value(&f.attrs[xi]), number_of(&f.attrs[yi])) else {
                        *left_out += 1;
                        continue;
                    };
                    pts[series_of(*r)].entry((x * 1e6).round() as i64).or_insert((x, Vec::new())).1.push(y);
                }
                let names = if series.is_empty() { vec![String::new()] } else { series.clone() };
                Panel::Lines(
                    names
                        .into_iter()
                        .zip(pts)
                        .filter(|s| !s.1.is_empty())
                        .map(|(n, m)| (n, m.into_values().map(|(x, ys)| (x, combined(&ys, &combine))).collect()))
                        .collect(),
                )
            }
        })
    };
    if kind == "pyramid" && series.len() != 2 {
        return err(format!(
            "A population pyramid needs an attribute under A line for each with two values, such as the sex, one for each side; {} has {}.",
            p.get("series").unwrap_or_else(|| "nothing chosen".into()),
            series.len()
        ));
    }
    let panels: Vec<(String, Panel)> = match field("facet")? {
        Some(fi) => {
            let mut names = first_seen(table, &all, fi);
            names.sort_by_cached_key(|b| (alphabetical(b), b.clone()));
            if names.len() > 36 {
                ctx.notes.warn(format!("{} small charts are too many to read; the first 36 are drawn.", names.len()));
                names.truncate(36);
            }
            names
                .into_iter()
                .map(|n| {
                    let rows: Vec<usize> = all.iter().copied().filter(|r| table.features[*r].attrs[fi].to_text() == n).collect();
                    Ok((n, build(&rows, &mut left_out)?))
                })
                .collect::<Result<_>>()?
        }
        None => vec![(String::new(), build(&all, &mut left_out)?)],
    };
    if left_out > 0 {
        ctx.notes.info(format!("{} features without a number left out", fmt_count(left_out as u64)));
    }
    let width = number_param(p, "width", "The width")?.unwrap_or(1200.0);
    if !(200.0..=8000.0).contains(&width) {
        return err("The width must be from 200 to 8000 pixels.");
    }
    let dates = kind == "lines" && field("x")?.is_some_and(|xi| table.features.iter().any(|f| x_value(&f.attrs[xi]).is_some_and(|x| x.1)));
    let plot = Plot {
        series: if kind == "bars" || kind == "box" || kind == "histogram" { Vec::new() } else { series },
        title: p.exact("title").trim().to_string(),
        subtitle: p.exact("subtitle").trim().to_string(),
        note: p.exact("note").trim().to_string(),
        unit: p.exact("unit").trim().to_string(),
        x_title: p.exact("x_title").trim().to_string(),
        decimals: number_param(p, "decimals", "The decimals")?.unwrap_or(0.0).clamp(0.0, 6.0) as usize,
        swedish: p.get("format").as_deref() != Some("en"),
        bins: number_param(p, "bins", "The bins")?.map_or(0, |b| b.clamp(2.0, 200.0) as usize),
        trend: p.get("trend").as_deref() != Some("no"),
        grouped: kind == "grouped",
        percent: p.get("percent").as_deref() == Some("yes"),
        dates,
        free_scales: p.get("free_scales").as_deref() == Some("yes"),
        markers: p.get("markers").as_deref() == Some("yes"),
        colour: chart::colour(&p.get("colour").unwrap_or_default()),
        width: width as u32,
        height: number_param(p, "height", "The height")?.map(|h| h.clamp(150.0, 20000.0) as u32),
        background: match p.get("background").as_deref() {
            Some("white") => Some([255, 255, 255]),
            Some("none") => None,
            _ => Some(chart::PAPER),
        },
        panels,
        ..Default::default()
    };
    if plot.series.len() > 9 && matches!(kind, "stacked" | "grouped" | "scatter") {
        ctx.notes.warn(format!("{} parts are more than there are colours to tell apart; some share a colour.", plot.series.len()));
    }
    if p.get("html").is_some_and(|h| !h.trim().is_empty()) {
        ctx.notes.warn("The web page is made for a single bar chart, so none was written for this chart.");
    }
    if plot.series.len() > 1 {
        let colours: Vec<[u8; 3]> = crate::plot::palette(plot.colour).into_iter().cycle().take(plot.series.len()).collect();
        if let Some((i, j, kind)) = crate::colourvision::confused(&colours) {
            ctx.notes.warn(format!(
                "{} and {} have colours that are hard to tell apart for those who are {}, about one man in twelve. Another first colour, or fewer parts, helps.",
                plot.series[i], plot.series[j], kind
            ));
        }
    }
    let image = crate::plot::draw(&plot)?;
    let n = plot.panels.len();
    ctx.notes.info(format!(
        "{}{} · {} × {} pixels",
        KINDS.iter().find(|k| k.0 == kind).map_or(kind, |k| k.1).split(':').next().unwrap_or(kind),
        if n > 1 { format!(", {} small charts", n) } else { String::new() },
        image.width,
        image.height
    ));
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
        desc: "Draws a bar chart of the features: a bar for each name, as long as its value, such as the area of each municipality. Or a line chart, with a line for each value of an attribute through its points along a number or a date, such as the inhabitants of each municipality year by year. Or a histogram of how values are spread, a scatter plot of one number against another with a trend line and its R², box plots of the spread in each group, stacked or grouped bars of the parts of each, or a population pyramid. Any of them can be drawn as small multiples: a small chart for each municipality, say, side by side on one scale. Sorted by size or by name, with a title, an explanation and a note. The chart comes out as a picture to write to a .png or a .pdf, and can also be written as an interactive web page, where each value shows under the pointer and the bars can be sorted.",
        inputs: &["Input"],
        outputs: &["Chart"],
        ports: None,
        params: CHART_PARAMS,
        run: run_chart,
        summary: |p| {
            let each = p.get("facet").filter(|f| !f.is_empty()).map(|f| format!(" · for each {}", f)).unwrap_or_default();
            match p.get("kind").as_deref() {
                Some("lines") => return format!("lines of {} along {}{}", p.get("value").unwrap_or_default(), p.get("x").unwrap_or_default(), each),
                Some("histogram") => return format!("histogram of {}{}", p.get("value").unwrap_or_default(), each),
                Some("scatter") => return format!("{} against {}{}", p.get("value").unwrap_or_default(), p.get("x").unwrap_or_default(), each),
                Some("box") => return format!("spread of {} by {}{}", p.get("value").unwrap_or_default(), p.get("category").unwrap_or_default(), each),
                Some(k @ ("stacked" | "grouped" | "pyramid")) => {
                    return format!("{} {} per {} and {}{}", k, p.get("value").filter(|v| !v.is_empty()).unwrap_or_else(|| "count".into()), p.get("category").unwrap_or_default(), p.get("series").unwrap_or_default(), each)
                }
                _ => {}
            }
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
    use super::*;
    use crate::model::{Feature, Field, FieldType};

    #[test]
    fn lines_are_drawn_for_each_series_along_years_or_dates() {
        let mut t = Table {
            fields: vec![Field::new("year", FieldType::Text), Field::new("place", FieldType::Text), Field::new("people", FieldType::Int)],
            ..Default::default()
        };
        for (y, pl, n) in [("2020", "Borås", 100), ("2021", "Borås", 110), ("2022", "Borås", 120), ("2020", "Skövde", 50), ("2022", "Skövde", 70)] {
            t.features.push(Feature::new(vec![Value::Text(y.into()), Value::Text(pl.into()), Value::Int(n)], None));
        }
        let (out, notes) = run_for_test(
            "ChartDrawer",
            &[("kind", "lines"), ("x", "year"), ("value", "people"), ("series", "place"), ("width", "600")],
            vec![("Input", t.clone())],
        )
        .unwrap();
        assert!(notes.info.iter().any(|n| n.starts_with("2 lines")), "{:?}", notes.info);
        let img = out[0].1.image.clone().unwrap();
        assert_eq!(img.width, 600);
        // Something is drawn in the colour of the first line.
        let blue = chart::colour("blue");
        assert!(img.rgba.chunks_exact(4).any(|p| p[0] == blue[0] && p[1] == blue[1] && p[2] == blue[2]));
        // Dates along the bottom.
        for f in t.features.iter_mut() {
            f.attrs[0] = Value::Text(format!("{}-06-01", f.attrs[0].to_text()));
        }
        let (out, _) = run_for_test("ChartDrawer", &[("kind", "lines"), ("x", "year"), ("value", "people"), ("series", "place")], vec![("Input", t)]).unwrap();
        assert!(out[0].1.image.is_some());
    }

    #[test]
    fn the_other_kinds_draw_and_small_multiples_split_by_an_attribute() {
        let mut t = Table {
            fields: vec![
                Field::new("place", FieldType::Text),
                Field::new("sex", FieldType::Text),
                Field::new("age", FieldType::Text),
                Field::new("people", FieldType::Int),
            ],
            ..Default::default()
        };
        for (pl, sx, a, n) in [
            ("Borås", "men", "0-9", 10),
            ("Borås", "women", "0-9", 11),
            ("Borås", "men", "10-19", 8),
            ("Borås", "women", "10-19", 9),
            ("Skövde", "men", "0-9", 5),
            ("Skövde", "women", "0-9", 4),
            ("Skövde", "men", "10-19", 6),
            ("Skövde", "women", "10-19", 7),
        ] {
            t.features.push(Feature::new(vec![Value::Text(pl.into()), Value::Text(sx.into()), Value::Text(a.into()), Value::Int(n)], None));
        }
        let base = [("category", "age"), ("series", "sex"), ("value", "people"), ("x", "people"), ("width", "600")];
        for kind in ["histogram", "scatter", "box", "stacked", "grouped", "pyramid"] {
            let mut ps = base.to_vec();
            ps.push(("kind", kind));
            let (out, notes) = run_for_test("ChartDrawer", &ps, vec![("Input", t.clone())]).unwrap_or_else(|e| panic!("{}: {}", kind, e));
            assert!(out[0].1.image.is_some(), "{}", kind);
            ps.push(("facet", "place"));
            let (_, notes2) = run_for_test("ChartDrawer", &ps, vec![("Input", t.clone())]).unwrap();
            assert!(notes2.info.iter().any(|n| n.contains("2 small charts")), "{} {:?} {:?}", kind, notes.info, notes2.info);
        }
        // Bars and lines as small multiples too.
        let (_, notes) = run_for_test("ChartDrawer", &[("category", "age"), ("value", "people"), ("facet", "place")], vec![("Input", t.clone())]).unwrap();
        assert!(notes.info.iter().any(|n| n.contains("2 small charts")), "{:?}", notes.info);
        // A pyramid needs two sides.
        assert!(run_for_test("ChartDrawer", &[("kind", "pyramid"), ("category", "age"), ("series", "place"), ("value", "people")], vec![("Input", t.clone())])
            .is_ok());
        assert!(
            run_for_test("ChartDrawer", &[("kind", "pyramid"), ("category", "age"), ("series", "people"), ("value", "people")], vec![("Input", t)]).is_err()
        );
    }

    #[test]
    fn swedish_letters_come_after_z() {
        let mut v = vec!["Öckerö", "Åmål", "Ale", "Ärla", "Zinkgruvan", "borås"];
        v.sort_by_key(|s| alphabetical(s));
        assert_eq!(v, ["Ale", "borås", "Zinkgruvan", "Åmål", "Ärla", "Öckerö"]);
    }
}
