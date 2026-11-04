//! Electronic nautical charts in S-57 (ENC): the cell, a .000 file, with its updates .001,
//! .002 and on applied. Each object class is a layer; read together, each feature says its
//! class by the code of S-57 and by a name to read, and soundings are points with their
//! depth.

use super::*;

/// The object classes of S-57 most charts have, by code, with a name to read.
pub(crate) const CLASSES: &[(&str, &str)] = &[
    ("ACHARE", "Anchorage area"),
    ("ACHBRT", "Anchor berth"),
    ("AIRARE", "Airport"),
    ("BCNCAR", "Beacon, cardinal"),
    ("BCNISD", "Beacon, isolated danger"),
    ("BCNLAT", "Beacon, lateral"),
    ("BCNSPP", "Beacon, special purpose"),
    ("BERTHS", "Berth"),
    ("BOYCAR", "Buoy, cardinal"),
    ("BOYISD", "Buoy, isolated danger"),
    ("BOYLAT", "Buoy, lateral"),
    ("BOYSAW", "Buoy, safe water"),
    ("BOYSPP", "Buoy, special purpose"),
    ("BRIDGE", "Bridge"),
    ("BUAARE", "Built-up area"),
    ("BUISGL", "Building"),
    ("CANALS", "Canal"),
    ("CAUSWY", "Causeway"),
    ("CBLOHD", "Cable, overhead"),
    ("CBLSUB", "Cable, submarine"),
    ("COALNE", "Coastline"),
    ("CTNARE", "Caution area"),
    ("DAMCON", "Dam"),
    ("DAYMAR", "Daymark"),
    ("DEPARE", "Depth area"),
    ("DEPCNT", "Depth contour"),
    ("DMPGRD", "Dumping ground"),
    ("DOCARE", "Dock area"),
    ("DRGARE", "Dredged area"),
    ("DYKCON", "Dyke"),
    ("FAIRWY", "Fairway"),
    ("FERYRT", "Ferry route"),
    ("FOGSIG", "Fog signal"),
    ("FSHFAC", "Fishing facility"),
    ("GATCON", "Gate"),
    ("HRBFAC", "Harbour facility"),
    ("ISTZNE", "Inshore traffic zone"),
    ("LAKARE", "Lake"),
    ("LIGHTS", "Light"),
    ("LNDARE", "Land area"),
    ("LNDELV", "Land elevation"),
    ("LNDMRK", "Landmark"),
    ("LNDRGN", "Land region"),
    ("LOKBSN", "Lock basin"),
    ("MAGVAR", "Magnetic variation"),
    ("MARCUL", "Marine farm"),
    ("MIPARE", "Military practice area"),
    ("MORFAC", "Mooring facility"),
    ("NAVLNE", "Navigation line"),
    ("OBSTRN", "Obstruction"),
    ("OFSPLF", "Offshore platform"),
    ("PILPNT", "Pile"),
    ("PIPOHD", "Pipeline, overhead"),
    ("PIPSOL", "Pipeline, submarine"),
    ("PONTON", "Pontoon"),
    ("PRCARE", "Precautionary area"),
    ("RAILWY", "Railway"),
    ("RECTRC", "Recommended track"),
    ("RESARE", "Restricted area"),
    ("RIVERS", "River"),
    ("ROADWY", "Road"),
    ("RTPBCN", "Radar transponder beacon"),
    ("RUNWAY", "Runway"),
    ("SBDARE", "Seabed area"),
    ("SEAARE", "Sea area"),
    ("SILTNK", "Silo or tank"),
    ("SLCONS", "Shoreline construction"),
    ("SLOTOP", "Slope topline"),
    ("SOUNDG", "Sounding"),
    ("SPLARE", "Sea-plane landing area"),
    ("TOPMAR", "Topmark"),
    ("TSELNE", "Traffic separation line"),
    ("TSSBND", "Traffic separation scheme boundary"),
    ("TSSLPT", "Traffic separation scheme lane"),
    ("TWRTPT", "Two-way route"),
    ("UNSARE", "Unsurveyed area"),
    ("UWTROC", "Underwater rock"),
    ("VEGATN", "Vegetation"),
    ("WRECKS", "Wreck"),
    ("M_COVR", "Coverage of the chart"),
    ("M_NPUB", "Nautical publication information"),
    ("M_QUAL", "Quality of data"),
    ("M_NSYS", "Navigational system of marks"),
    ("M_SREL", "Survey reliability"),
    ("C_AGGR", "Aggregation"),
    ("C_ASSO", "Association"),
];

/// The name to read of an object class, or its code when it is not among the common ones.
pub(crate) fn class_name(code: &str) -> String {
    CLASSES.iter().find(|c| c.0 == code).map_or_else(|| code.to_string(), |c| c.1.to_string())
}

thread_local! {
    /// While a class of a cell is read as a plain layer, by the reading of the whole cell.
    static PLAIN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Is a cell to be read as a chart here, not as a plain layer for the reading of a chart.
pub(crate) fn as_chart(path: &Path) -> bool {
    is_cell(path) && !PLAIN.with(|p| p.get())
}

/// Is this an S-57 cell: a file whose extension is three digits, as .000.
pub(crate) fn is_cell(path: &Path) -> bool {
    let ext = ext_of(path);
    ext.len() == 3 && ext.chars().all(|c| c.is_ascii_digit())
}

/// Reads a cell. Without a layer chosen, every object class, as one stream; with one, that
/// class. Updates beside the cell are applied, texts in national characters are read as
/// they are meant, and each sounding is a point of its own with its depth.
pub(crate) fn read(backend: &GdalBackend, path: &Path, opts: &ReadOpts, notes: &mut Notes, progress: &dyn Fn(u64, Option<u64>) -> bool) -> Result<Table> {
    let base = if ext_of(path) == "000" {
        path.to_path_buf()
    } else {
        // An update file alone cannot be read: the cell it updates is.
        let cell = path.with_extension("000");
        if !cell.exists() {
            return Err(GbError(format!("{} is an update of an S-57 cell, and the cell {} is not beside it.", path.display(), cell.display())));
        }
        notes.info(format!("{} is an update; the cell {} is read with its updates", path.display(), cell.display()));
        cell
    };
    let _ = gdal::config::set_thread_local_config_option(
        "OGR_S57_OPTIONS",
        "RETURN_PRIMITIVES=OFF,RETURN_LINKAGES=OFF,LNAM_REFS=ON,SPLIT_MULTIPOINT=ON,ADD_SOUNDG_DEPTH=ON,UPDATES=APPLY,RECODE_BY_DSSI=ON",
    );
    let result = (|| {
        let ds = open(&base, false, &[])?;
        let layers: Vec<String> = match opts.layer.as_deref().filter(|l| !l.trim().is_empty()) {
            Some(one) => vec![one.to_string()],
            // DSID describes the cell itself, with no place on the chart.
            None => ds.layers().map(|l| l.name()).filter(|n| n != "DSID").collect(),
        };
        drop(ds);
        let mut tables = Vec::new();
        for code in &layers {
            let one = ReadOpts { layer: Some(code.clone()), crs: opts.crs.or(Some(4326)), ..opts.clone() };
            let mut t = PLAIN.with(|p| {
                p.set(true);
                let t = backend.read_with(&base, &one, notes, progress);
                p.set(false);
                t
            })?;
            if t.is_empty() {
                continue;
            }
            // The class, by code and by name, first among the attributes.
            t.fields.insert(0, Field::new("object", FieldType::Text));
            t.fields.insert(0, Field::new("object_class", FieldType::Text));
            for f in t.features.iter_mut() {
                f.attrs.insert(0, Value::Text(class_name(code)));
                f.attrs.insert(0, Value::Text(code.clone()));
            }
            tables.push(std::sync::Arc::new(t));
        }
        if tables.is_empty() {
            return Ok(Table {
                crs: Some(4326),
                fields: vec![Field::new("object_class", FieldType::Text), Field::new("object", FieldType::Text)],
                ..Default::default()
            });
        }
        let n = tables.len();
        let merged = og_engine::nodes::merge_tables(tables, Some(backend as &dyn Backend), "object classes", notes)?;
        if opts.layer.is_none() {
            notes.info(format!("an S-57 chart: {} object classes; choose one under Layers to read only it", n));
        }
        Ok(std::sync::Arc::try_unwrap(merged).unwrap_or_else(|t| (*t).clone()))
    })();
    let _ = gdal::config::clear_thread_local_config_option("OGR_S57_OPTIONS");
    result
}
