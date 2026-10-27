//! Workbooks written by the program itself, not by GDAL, so that they can be styled: a
//! font, a bold top row that stays in place, columns as wide as their contents, and colour
//! schemes where every second row is tinted. GDAL still reads them.
//!
//! A workbook that is already there is opened and only the sheet with the same name is
//! replaced. The other sheets are kept as they are, with their formatting.

use og_engine::io::{FieldSize, Notes, SheetStyle};
use og_engine::{FieldType, GbError, Result, Table, Value};
use std::collections::HashMap;
use std::path::Path;

/// The colours of a scheme, as RGB: the top row, its text, and every second row below it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scheme {
    pub header: [u8; 3],
    pub header_text: [u8; 3],
    pub band: [u8; 3],
}

/// The colour schemes by the names the writer stores. Each has a dark top row with white
/// text and a light tint of the same hue on every second row.
pub fn scheme(name: &str) -> Option<Scheme> {
    let white = [255, 255, 255];
    let (header, band) = match name {
        "grey" => ([0x45, 0x50, 0x4e], [0xf1, 0xf3, 0xf2]),
        "blue" => ([0x1f, 0x4e, 0x79], [0xe9, 0xf0, 0xf7]),
        "green" => ([0x2e, 0x6b, 0x45], [0xe9, 0xf4, 0xed]),
        "sand" => ([0x7a, 0x5c, 0x1e], [0xf8, 0xf1, 0xe2]),
        "magenta" => ([0xa3, 0x17, 0x5c], [0xf9, 0xe9, 0xf1]),
        _ => return None,
    };
    Some(Scheme { header, header_text: white, band })
}

fn hex(c: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// How wide each column is to be, in characters: as wide as the name or the longest value
/// among the first thousand rows, within reason.
fn widths(table: &Table) -> Vec<f64> {
    table
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let longest = table.features.iter().take(1000).map(|x| shown(&x.attrs[i]).chars().count()).max().unwrap_or(0);
            // A bold name is a little wider than the same text in the body.
            let name = f.name.chars().count() as f64 * 1.1;
            (name.max(longest as f64) + 2.0).clamp(6.0, 60.0)
        })
        .collect()
}

/// A value as it will be seen in the cell, to measure it.
fn shown(v: &Value) -> String {
    match v {
        Value::Real(x) => format!("{}", (x * 1e6).round() / 1e6),
        other => other.to_text(),
    }
}

/// What goes into a cell: a number, a text, or nothing.
enum Cell<'a> {
    Number(f64),
    Text(std::borrow::Cow<'a, str>),
    Empty,
}

fn cell_of<'a>(v: &'a Value, ty: FieldType) -> Cell<'a> {
    match (v, ty) {
        (Value::Null, _) => Cell::Empty,
        (Value::Int(i), _) => Cell::Number(*i as f64),
        (Value::Real(x), _) if x.is_finite() => Cell::Number(*x),
        (Value::Real(_), _) => Cell::Empty,
        (Value::Bool(b), _) => Cell::Text(if *b { "true".into() } else { "false".into() }),
        (Value::Text(s), _) => Cell::Text(s.as_str().into()),
    }
}

/// The number format of a column, from the number of decimals the writer was told.
fn decimals_of(table: &Table, sizes: &HashMap<String, FieldSize>) -> Vec<Option<u32>> {
    table.fields.iter().map(|f| if f.ty == FieldType::Real { sizes.get(&f.name).and_then(|s| s.decimals).map(|d| d.min(15)) } else { None }).collect()
}

fn is_number(ty: FieldType) -> bool {
    matches!(ty, FieldType::Int | FieldType::Real)
}

fn locked(path: &Path, e: impl std::fmt::Display) -> GbError {
    GbError(format!("Cannot save {}: {}. Is the file open in Excel or LibreOffice?", path.display(), e))
}

// ---------- Where the columns go ----------

/// The column of each attribute, from 0, when rows are added under a header that is there:
/// the column with the same name, or a new one after the last. Also the new columns.
fn columns_under(table: &Table, header: &[String]) -> (Vec<u32>, Vec<usize>) {
    let mut cols = Vec::new();
    let mut added = Vec::new();
    let mut next = header.len() as u32;
    for (i, f) in table.fields.iter().enumerate() {
        match header.iter().position(|h| h.trim() == f.name) {
            Some(c) => cols.push(c as u32),
            None => {
                cols.push(next);
                next += 1;
                added.push(i);
            }
        }
    }
    (cols, added)
}

fn note_added(table: &Table, added: &[usize], sheet: &str, notes: &mut Notes) {
    if !added.is_empty() {
        let names: Vec<&str> = added.iter().map(|i| table.fields[*i].name.as_str()).collect();
        notes.info(format!("new columns in {}: {}", sheet, names.join(", ")));
    }
}

/// Whether a row of data is tinted in a colour scheme. The rows are counted from the first
/// row under the names, so that rows that are added go on in the same rhythm.
fn banded(data_row: u32) -> bool {
    data_row % 2 == 1
}

// ---------- Excel ----------

fn xlsx_font(s: &mut umya_spreadsheet::Style, style: &SheetStyle, bold: bool, color: Option<[u8; 3]>) {
    if style.font.is_none() && !bold && color.is_none() {
        return;
    }
    let f = s.font_mut();
    if let Some(name) = &style.font {
        f.set_name(name.as_str());
    }
    if bold {
        f.set_bold(true);
    }
    if let Some(c) = color {
        f.color_mut().set_argb_str(format!("FF{}", hex(c)));
    }
}

/// The name of a column in the top row.
fn xlsx_header(ws: &mut umya_spreadsheet::Worksheet, col: u32, name: &str, ty: FieldType, style: &SheetStyle) {
    let scheme = style.colors.as_deref().and_then(scheme);
    let cell = ws.cell_mut((col + 1, 1));
    cell.set_value_string(name);
    let s = cell.style_mut();
    xlsx_font(s, style, style.bold_header, scheme.map(|x| x.header_text));
    if let Some(x) = scheme {
        s.set_background_color_solid(format!("FF{}", hex(x.header)));
    }
    // The name of a column of numbers stands over the numbers, to the right.
    if is_number(ty) {
        s.alignment_mut().set_horizontal(umya_spreadsheet::structs::HorizontalAlignmentValues::Right);
    }
}

/// The features as rows, from `first` (counted from 0 under the names), in the columns given.
fn xlsx_rows(ws: &mut umya_spreadsheet::Worksheet, table: &Table, cols: &[u32], first: u32, style: &SheetStyle, decimals: &[Option<u32>]) {
    let scheme = style.colors.as_deref().and_then(scheme);
    for (r, feature) in table.features.iter().enumerate() {
        let data_row = first + r as u32;
        let tint = scheme.filter(|_| banded(data_row));
        for (c, field) in table.fields.iter().enumerate() {
            let value = cell_of(&feature.attrs[c], field.ty);
            // Empty cells are only made when they are to be coloured.
            if matches!(value, Cell::Empty) && tint.is_none() {
                continue;
            }
            let cell = ws.cell_mut((cols[c] + 1, data_row + 2));
            match value {
                Cell::Number(x) => {
                    cell.set_value_number(x);
                }
                Cell::Text(t) => {
                    cell.set_value_string(t.as_ref());
                }
                Cell::Empty => {}
            }
            let s = cell.style_mut();
            xlsx_font(s, style, false, None);
            if let Some(x) = tint {
                s.set_background_color_solid(format!("FF{}", hex(x.band)));
            }
            if let Some(d) = decimals[c] {
                let code = if d == 0 { "0".to_string() } else { format!("0.{}", "0".repeat(d as usize)) };
                s.number_format_mut().set_format_code(code);
            }
        }
    }
}

pub fn write_xlsx(
    table: &Table,
    path: &Path,
    sheet: &str,
    sizes: &HashMap<String, FieldSize>,
    style: &SheetStyle,
    append: bool,
    notes: &mut Notes,
) -> Result<()> {
    use umya_spreadsheet::structs::{Pane, PaneStateValues, PaneValues, SheetView, SheetViews};
    let mut book = if path.exists() {
        umya_spreadsheet::reader::xlsx::read(path)
            .map_err(|e| GbError(format!("Cannot open {} to replace a sheet in it: {}. Is the file open in Excel?", path.display(), e)))?
    } else {
        umya_spreadsheet::new_file_empty_worksheet()
    };
    let names: Vec<String> = book.sheet_collection_no_check().iter().map(|s| s.name().to_string()).collect();
    let place = names.iter().position(|n| n.eq_ignore_ascii_case(sheet));
    let decimals = decimals_of(table, sizes);

    let at = match (append, place) {
        // Rows are added under those that are there, in the columns with the same names.
        (true, Some(i)) => {
            let ws = book.sheet_mut(i).map_err(|e| GbError(format!("Cannot open the sheet {}: {}", sheet, e)))?;
            let mut header = Vec::new();
            while let Some(v) = ws.cell((header.len() as u32 + 1, 1)).map(|c| c.value().to_string()).filter(|v| !v.is_empty()) {
                header.push(v);
            }
            let used = ws.highest_row().max(1);
            let (cols, added) = columns_under(table, &header);
            for k in &added {
                xlsx_header(ws, cols[*k], &table.fields[*k].name, table.fields[*k].ty, style);
            }
            xlsx_rows(ws, table, &cols, used - 1, style, &decimals);
            for (k, w) in widths(table).into_iter().enumerate() {
                let dim = ws.column_dimension_by_number_mut(cols[k] + 1);
                if dim.width() < w {
                    dim.set_width(w);
                }
            }
            note_added(table, &added, sheet, notes);
            i
        }
        _ => {
            // A sheet with the same name, whatever the case, is replaced where it stands.
            if let Some(i) = place {
                book.remove_sheet(i).map_err(|e| GbError(format!("Cannot replace the sheet {}: {}", sheet, e)))?;
            }
            let ws = book.new_sheet(sheet).map_err(|e| GbError(format!("Cannot create the sheet {}: {}", sheet, e)))?;
            let cols: Vec<u32> = (0..table.fields.len() as u32).collect();
            for (c, field) in table.fields.iter().enumerate() {
                xlsx_header(ws, c as u32, &field.name, field.ty, style);
            }
            xlsx_rows(ws, table, &cols, 0, style, &decimals);
            for (c, w) in widths(table).into_iter().enumerate() {
                ws.column_dimension_by_number_mut(c as u32 + 1).set_width(w);
            }
            if style.freeze_header {
                let mut pane = Pane::default();
                pane.set_vertical_split(1.0);
                let mut corner = umya_spreadsheet::structs::Coordinate::default();
                corner.set_coordinate("A2");
                pane.set_top_left_cell(corner);
                pane.set_active_pane(PaneValues::BottomLeft);
                pane.set_state(PaneStateValues::Frozen);
                let mut view = SheetView::default();
                view.set_workbook_view_id(0);
                view.set_pane(pane);
                let mut views = SheetViews::default();
                views.add_sheet_view_list_mut(view);
                ws.set_sheets_views(views);
            }
            // Back in the place of the sheet it replaced.
            let last = book.sheet_count() - 1;
            let at = place.unwrap_or(last).min(last);
            book.sheet_collection_mut()[at..].rotate_right(1);
            at
        }
    };
    // The sheet that was written is the one shown when the file is opened.
    for (i, s) in book.sheet_collection_mut().iter_mut().enumerate() {
        for v in s.sheet_views_mut().sheet_view_list_mut() {
            v.set_tab_selected(i == at);
        }
    }
    book.set_active_sheet(at as u32);
    umya_spreadsheet::writer::xlsx::write(&book, path).map_err(|e| locked(path, e))
}

// ---------- LibreOffice ----------

pub fn write_ods(
    table: &Table,
    path: &Path,
    sheet: &str,
    sizes: &HashMap<String, FieldSize>,
    style: &SheetStyle,
    append: bool,
    notes: &mut Notes,
) -> Result<()> {
    use spreadsheet_ods::color::Rgb;
    use spreadsheet_ods::defaultstyles::DefaultFormat;
    use spreadsheet_ods::format::create_number_format_fixed;
    use spreadsheet_ods::style::FontFaceDecl;
    use spreadsheet_ods::{CellStyle, Length, Sheet, WorkBook};

    let mut book = if path.exists() {
        spreadsheet_ods::read_ods(path)
            .map_err(|e| GbError(format!("Cannot open {} to replace a sheet in it: {}. Is the file open in LibreOffice?", path.display(), e)))?
    } else {
        WorkBook::new(icu_locale_core::locale!("sv-SE"))
    };
    let place = (0..book.num_sheets()).find(|i| book.sheet(*i).name().eq_ignore_ascii_case(sheet));
    let appending = append && place.is_some();
    if let (false, Some(i)) = (appending, place) {
        book.remove_sheet(i);
    }
    let scheme = style.colors.as_deref().and_then(scheme);
    let rgb = |c: [u8; 3]| Rgb::new(c[0], c[1], c[2]);
    if let Some(name) = &style.font {
        if book.font(name).is_none() {
            let mut decl = FontFaceDecl::new(name);
            decl.set_font_family(name.as_str());
            book.add_font(decl);
        }
    }
    // The styles are named after the sheet, so that sheets with other looks keep theirs.
    let key: String = sheet.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
    let mut make = |name: &str, header: bool, band: bool, decimals: Option<u32>, right: bool| {
        let format = match decimals {
            Some(d) => {
                let f = create_number_format_fixed(format!("gb_{}_n{}", key, d), d as u8, false);
                book.add_number_format(f)
            }
            None => DefaultFormat::default(),
        };
        let mut s = CellStyle::new(format!("gb_{}_{}", key, name), &format);
        if let Some(font) = &style.font {
            s.set_font_name(font.as_str());
        }
        if right {
            s.set_text_align(spreadsheet_ods::style::units::TextAlign::End);
        }
        if header {
            if style.bold_header {
                s.set_font_bold();
            }
            if let Some(x) = scheme {
                s.set_background_color(rgb(x.header));
                s.set_color(rgb(x.header_text));
            }
        } else if let Some(x) = scheme.filter(|_| band) {
            s.set_background_color(rgb(x.band));
        }
        book.add_cellstyle(s)
    };
    let header_style = make("head", true, false, None, false);
    let number_header_style = make("head_number", true, false, None, true);
    let decimals = decimals_of(table, sizes);
    // One style for each kind of cell: plain or tinted, and with a number of decimals or not.
    let mut body = HashMap::new();
    for band in [false, true] {
        for d in decimals.iter().copied().chain([None]) {
            body.entry((band, d)).or_insert_with(|| {
                let name = format!("{}{}", if band { "band" } else { "row" }, d.map(|d| format!("_d{}", d)).unwrap_or_default());
                make(&name, false, band, d, false)
            });
        }
    }
    let styled = style.font.is_some() || scheme.is_some() || decimals.iter().any(|d| d.is_some());
    let header_styled = style.bold_header || scheme.is_some() || style.font.is_some();
    let put_header = |ws: &mut Sheet, col: u32, name: &str, ty: FieldType| {
        if header_styled {
            let s = if is_number(ty) { &number_header_style } else { &header_style };
            ws.set_styled_value(0, col, name, s);
        } else {
            ws.set_value(0, col, name);
        }
    };
    let put_rows = |ws: &mut Sheet, cols: &[u32], first: u32| {
        for (r, feature) in table.features.iter().enumerate() {
            let data_row = first + r as u32;
            let band = scheme.is_some() && banded(data_row);
            for (c, field) in table.fields.iter().enumerate() {
                let value = cell_of(&feature.attrs[c], field.ty);
                let (row, col) = (data_row + 1, cols[c]);
                let s = &body[&(band, decimals[c])];
                match value {
                    Cell::Number(x) if styled => ws.set_styled_value(row, col, x, s),
                    Cell::Number(x) => ws.set_value(row, col, x),
                    Cell::Text(t) if styled => ws.set_styled_value(row, col, t.into_owned(), s),
                    Cell::Text(t) => ws.set_value(row, col, t.into_owned()),
                    // An empty cell is written too, so that every row is as long as the top row.
                    // GDAL takes the top row for names only when the rows below match it.
                    Cell::Empty => ws.set_cellstyle(row, col, s),
                }
            }
        }
    };

    if let (true, Some(i)) = (appending, place) {
        let ws = book.sheet_mut(i);
        let mut header = Vec::new();
        while let spreadsheet_ods::Value::Text(t) = ws.value(0, header.len() as u32) {
            header.push(t.clone());
        }
        let used = ws.used_grid_size().0.max(1);
        let (cols, added) = columns_under(table, &header);
        for k in &added {
            put_header(ws, cols[*k], &table.fields[*k].name, table.fields[*k].ty);
        }
        put_rows(ws, &cols, used - 1);
        note_added(table, &added, sheet, notes);
    } else {
        let mut ws = Sheet::new(sheet);
        for (c, field) in table.fields.iter().enumerate() {
            put_header(&mut ws, c as u32, &field.name, field.ty);
        }
        let cols: Vec<u32> = (0..table.fields.len() as u32).collect();
        put_rows(&mut ws, &cols, 0);
        for (c, w) in widths(table).into_iter().enumerate() {
            // A character of the default font is about two millimetres wide.
            ws.set_col_width(c as u32, Length::Cm(w * 0.2));
        }
        if style.freeze_header {
            ws.split_row_header(0);
        }
        let at = place.unwrap_or(book.num_sheets()).min(book.num_sheets());
        book.insert_sheet(at, ws);
    }
    book.config_mut().active_table = sheet.to_string();
    spreadsheet_ods::write_ods(&mut book, path).map_err(|e| locked(path, e))
}
