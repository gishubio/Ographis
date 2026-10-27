//! The map icons of Maki, by Mapbox, under CC0 (icons/maki/LICENSE.txt): small pictures
//! for points of interest, such as a hospital, a police station or a charging station,
//! drawn on maps by Draw map. Words from OpenStreetMap and Swedish ones lead to them too.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use tiny_skia::Pixmap;

/// The icons, by name, as SVG of 15 × 15.
pub const MAKI: &[(&str, &str)] = &[
    ("aerialway", include_str!("../icons/maki/aerialway.svg")),
    ("airfield", include_str!("../icons/maki/airfield.svg")),
    ("airport", include_str!("../icons/maki/airport.svg")),
    ("alcohol-shop", include_str!("../icons/maki/alcohol-shop.svg")),
    ("american-football", include_str!("../icons/maki/american-football.svg")),
    ("amusement-park", include_str!("../icons/maki/amusement-park.svg")),
    ("animal-shelter", include_str!("../icons/maki/animal-shelter.svg")),
    ("aquarium", include_str!("../icons/maki/aquarium.svg")),
    ("arrow", include_str!("../icons/maki/arrow.svg")),
    ("art-gallery", include_str!("../icons/maki/art-gallery.svg")),
    ("attraction", include_str!("../icons/maki/attraction.svg")),
    ("bakery", include_str!("../icons/maki/bakery.svg")),
    ("bank", include_str!("../icons/maki/bank.svg")),
    ("bank-JP", include_str!("../icons/maki/bank-JP.svg")),
    ("bar", include_str!("../icons/maki/bar.svg")),
    ("barrier", include_str!("../icons/maki/barrier.svg")),
    ("baseball", include_str!("../icons/maki/baseball.svg")),
    ("basketball", include_str!("../icons/maki/basketball.svg")),
    ("bbq", include_str!("../icons/maki/bbq.svg")),
    ("beach", include_str!("../icons/maki/beach.svg")),
    ("beer", include_str!("../icons/maki/beer.svg")),
    ("bicycle", include_str!("../icons/maki/bicycle.svg")),
    ("bicycle-share", include_str!("../icons/maki/bicycle-share.svg")),
    ("blood-bank", include_str!("../icons/maki/blood-bank.svg")),
    ("bowling-alley", include_str!("../icons/maki/bowling-alley.svg")),
    ("bridge", include_str!("../icons/maki/bridge.svg")),
    ("building", include_str!("../icons/maki/building.svg")),
    ("building-alt1", include_str!("../icons/maki/building-alt1.svg")),
    ("bus", include_str!("../icons/maki/bus.svg")),
    ("cafe", include_str!("../icons/maki/cafe.svg")),
    ("campsite", include_str!("../icons/maki/campsite.svg")),
    ("car", include_str!("../icons/maki/car.svg")),
    ("car-rental", include_str!("../icons/maki/car-rental.svg")),
    ("car-repair", include_str!("../icons/maki/car-repair.svg")),
    ("casino", include_str!("../icons/maki/casino.svg")),
    ("castle", include_str!("../icons/maki/castle.svg")),
    ("castle-JP", include_str!("../icons/maki/castle-JP.svg")),
    ("caution", include_str!("../icons/maki/caution.svg")),
    ("cemetery", include_str!("../icons/maki/cemetery.svg")),
    ("cemetery-JP", include_str!("../icons/maki/cemetery-JP.svg")),
    ("charging-station", include_str!("../icons/maki/charging-station.svg")),
    ("cinema", include_str!("../icons/maki/cinema.svg")),
    ("circle", include_str!("../icons/maki/circle.svg")),
    ("circle-stroked", include_str!("../icons/maki/circle-stroked.svg")),
    ("city", include_str!("../icons/maki/city.svg")),
    ("clothing-store", include_str!("../icons/maki/clothing-store.svg")),
    ("college", include_str!("../icons/maki/college.svg")),
    ("college-JP", include_str!("../icons/maki/college-JP.svg")),
    ("commercial", include_str!("../icons/maki/commercial.svg")),
    ("communications-tower", include_str!("../icons/maki/communications-tower.svg")),
    ("confectionery", include_str!("../icons/maki/confectionery.svg")),
    ("construction", include_str!("../icons/maki/construction.svg")),
    ("convenience", include_str!("../icons/maki/convenience.svg")),
    ("cricket", include_str!("../icons/maki/cricket.svg")),
    ("cross", include_str!("../icons/maki/cross.svg")),
    ("dam", include_str!("../icons/maki/dam.svg")),
    ("danger", include_str!("../icons/maki/danger.svg")),
    ("defibrillator", include_str!("../icons/maki/defibrillator.svg")),
    ("dentist", include_str!("../icons/maki/dentist.svg")),
    ("diamond", include_str!("../icons/maki/diamond.svg")),
    ("doctor", include_str!("../icons/maki/doctor.svg")),
    ("dog-park", include_str!("../icons/maki/dog-park.svg")),
    ("drinking-water", include_str!("../icons/maki/drinking-water.svg")),
    ("elevator", include_str!("../icons/maki/elevator.svg")),
    ("embassy", include_str!("../icons/maki/embassy.svg")),
    ("emergency-phone", include_str!("../icons/maki/emergency-phone.svg")),
    ("entrance", include_str!("../icons/maki/entrance.svg")),
    ("entrance-alt1", include_str!("../icons/maki/entrance-alt1.svg")),
    ("farm", include_str!("../icons/maki/farm.svg")),
    ("fast-food", include_str!("../icons/maki/fast-food.svg")),
    ("fence", include_str!("../icons/maki/fence.svg")),
    ("ferry", include_str!("../icons/maki/ferry.svg")),
    ("ferry-JP", include_str!("../icons/maki/ferry-JP.svg")),
    ("fire-station", include_str!("../icons/maki/fire-station.svg")),
    ("fire-station-JP", include_str!("../icons/maki/fire-station-JP.svg")),
    ("fitness-centre", include_str!("../icons/maki/fitness-centre.svg")),
    ("florist", include_str!("../icons/maki/florist.svg")),
    ("fuel", include_str!("../icons/maki/fuel.svg")),
    ("furniture", include_str!("../icons/maki/furniture.svg")),
    ("gaming", include_str!("../icons/maki/gaming.svg")),
    ("garden", include_str!("../icons/maki/garden.svg")),
    ("garden-centre", include_str!("../icons/maki/garden-centre.svg")),
    ("gate", include_str!("../icons/maki/gate.svg")),
    ("gift", include_str!("../icons/maki/gift.svg")),
    ("globe", include_str!("../icons/maki/globe.svg")),
    ("golf", include_str!("../icons/maki/golf.svg")),
    ("grocery", include_str!("../icons/maki/grocery.svg")),
    ("hairdresser", include_str!("../icons/maki/hairdresser.svg")),
    ("harbor", include_str!("../icons/maki/harbor.svg")),
    ("hardware", include_str!("../icons/maki/hardware.svg")),
    ("heart", include_str!("../icons/maki/heart.svg")),
    ("heliport", include_str!("../icons/maki/heliport.svg")),
    ("highway-rest-area", include_str!("../icons/maki/highway-rest-area.svg")),
    ("historic", include_str!("../icons/maki/historic.svg")),
    ("home", include_str!("../icons/maki/home.svg")),
    ("horse-riding", include_str!("../icons/maki/horse-riding.svg")),
    ("hospital", include_str!("../icons/maki/hospital.svg")),
    ("hospital-JP", include_str!("../icons/maki/hospital-JP.svg")),
    ("hot-spring", include_str!("../icons/maki/hot-spring.svg")),
    ("ice-cream", include_str!("../icons/maki/ice-cream.svg")),
    ("industry", include_str!("../icons/maki/industry.svg")),
    ("information", include_str!("../icons/maki/information.svg")),
    ("jewelry-store", include_str!("../icons/maki/jewelry-store.svg")),
    ("karaoke", include_str!("../icons/maki/karaoke.svg")),
    ("landmark", include_str!("../icons/maki/landmark.svg")),
    ("landmark-JP", include_str!("../icons/maki/landmark-JP.svg")),
    ("landuse", include_str!("../icons/maki/landuse.svg")),
    ("laundry", include_str!("../icons/maki/laundry.svg")),
    ("library", include_str!("../icons/maki/library.svg")),
    ("lift-gate", include_str!("../icons/maki/lift-gate.svg")),
    ("lighthouse", include_str!("../icons/maki/lighthouse.svg")),
    ("lighthouse-JP", include_str!("../icons/maki/lighthouse-JP.svg")),
    ("lodging", include_str!("../icons/maki/lodging.svg")),
    ("logging", include_str!("../icons/maki/logging.svg")),
    ("marae", include_str!("../icons/maki/marae.svg")),
    ("marker", include_str!("../icons/maki/marker.svg")),
    ("marker-stroked", include_str!("../icons/maki/marker-stroked.svg")),
    ("mobile-phone", include_str!("../icons/maki/mobile-phone.svg")),
    ("monument", include_str!("../icons/maki/monument.svg")),
    ("monument-JP", include_str!("../icons/maki/monument-JP.svg")),
    ("mountain", include_str!("../icons/maki/mountain.svg")),
    ("museum", include_str!("../icons/maki/museum.svg")),
    ("music", include_str!("../icons/maki/music.svg")),
    ("natural", include_str!("../icons/maki/natural.svg")),
    ("nightclub", include_str!("../icons/maki/nightclub.svg")),
    ("observation-tower", include_str!("../icons/maki/observation-tower.svg")),
    ("optician", include_str!("../icons/maki/optician.svg")),
    ("paint", include_str!("../icons/maki/paint.svg")),
    ("park", include_str!("../icons/maki/park.svg")),
    ("park-alt1", include_str!("../icons/maki/park-alt1.svg")),
    ("parking", include_str!("../icons/maki/parking.svg")),
    ("parking-garage", include_str!("../icons/maki/parking-garage.svg")),
    ("parking-paid", include_str!("../icons/maki/parking-paid.svg")),
    ("pharmacy", include_str!("../icons/maki/pharmacy.svg")),
    ("picnic-site", include_str!("../icons/maki/picnic-site.svg")),
    ("pitch", include_str!("../icons/maki/pitch.svg")),
    ("place-of-worship", include_str!("../icons/maki/place-of-worship.svg")),
    ("playground", include_str!("../icons/maki/playground.svg")),
    ("police", include_str!("../icons/maki/police.svg")),
    ("police-JP", include_str!("../icons/maki/police-JP.svg")),
    ("post", include_str!("../icons/maki/post.svg")),
    ("post-JP", include_str!("../icons/maki/post-JP.svg")),
    ("prison", include_str!("../icons/maki/prison.svg")),
    ("racetrack", include_str!("../icons/maki/racetrack.svg")),
    ("racetrack-boat", include_str!("../icons/maki/racetrack-boat.svg")),
    ("racetrack-cycling", include_str!("../icons/maki/racetrack-cycling.svg")),
    ("racetrack-horse", include_str!("../icons/maki/racetrack-horse.svg")),
    ("rail", include_str!("../icons/maki/rail.svg")),
    ("rail-light", include_str!("../icons/maki/rail-light.svg")),
    ("rail-metro", include_str!("../icons/maki/rail-metro.svg")),
    ("ranger-station", include_str!("../icons/maki/ranger-station.svg")),
    ("recycling", include_str!("../icons/maki/recycling.svg")),
    ("religious-buddhist", include_str!("../icons/maki/religious-buddhist.svg")),
    ("religious-christian", include_str!("../icons/maki/religious-christian.svg")),
    ("religious-jewish", include_str!("../icons/maki/religious-jewish.svg")),
    ("religious-muslim", include_str!("../icons/maki/religious-muslim.svg")),
    ("religious-shinto", include_str!("../icons/maki/religious-shinto.svg")),
    ("residential-community", include_str!("../icons/maki/residential-community.svg")),
    ("restaurant", include_str!("../icons/maki/restaurant.svg")),
    ("restaurant-bbq", include_str!("../icons/maki/restaurant-bbq.svg")),
    ("restaurant-noodle", include_str!("../icons/maki/restaurant-noodle.svg")),
    ("restaurant-pizza", include_str!("../icons/maki/restaurant-pizza.svg")),
    ("restaurant-seafood", include_str!("../icons/maki/restaurant-seafood.svg")),
    ("restaurant-sushi", include_str!("../icons/maki/restaurant-sushi.svg")),
    ("road-accident", include_str!("../icons/maki/road-accident.svg")),
    ("roadblock", include_str!("../icons/maki/roadblock.svg")),
    ("rocket", include_str!("../icons/maki/rocket.svg")),
    ("school", include_str!("../icons/maki/school.svg")),
    ("school-JP", include_str!("../icons/maki/school-JP.svg")),
    ("scooter", include_str!("../icons/maki/scooter.svg")),
    ("shelter", include_str!("../icons/maki/shelter.svg")),
    ("shoe", include_str!("../icons/maki/shoe.svg")),
    ("shop", include_str!("../icons/maki/shop.svg")),
    ("skateboard", include_str!("../icons/maki/skateboard.svg")),
    ("skiing", include_str!("../icons/maki/skiing.svg")),
    ("slaughterhouse", include_str!("../icons/maki/slaughterhouse.svg")),
    ("slipway", include_str!("../icons/maki/slipway.svg")),
    ("snowmobile", include_str!("../icons/maki/snowmobile.svg")),
    ("soccer", include_str!("../icons/maki/soccer.svg")),
    ("square", include_str!("../icons/maki/square.svg")),
    ("square-stroked", include_str!("../icons/maki/square-stroked.svg")),
    ("stadium", include_str!("../icons/maki/stadium.svg")),
    ("star", include_str!("../icons/maki/star.svg")),
    ("star-stroked", include_str!("../icons/maki/star-stroked.svg")),
    ("suitcase", include_str!("../icons/maki/suitcase.svg")),
    ("swimming", include_str!("../icons/maki/swimming.svg")),
    ("table-tennis", include_str!("../icons/maki/table-tennis.svg")),
    ("taxi", include_str!("../icons/maki/taxi.svg")),
    ("teahouse", include_str!("../icons/maki/teahouse.svg")),
    ("telephone", include_str!("../icons/maki/telephone.svg")),
    ("tennis", include_str!("../icons/maki/tennis.svg")),
    ("terminal", include_str!("../icons/maki/terminal.svg")),
    ("theatre", include_str!("../icons/maki/theatre.svg")),
    ("toilet", include_str!("../icons/maki/toilet.svg")),
    ("toll", include_str!("../icons/maki/toll.svg")),
    ("town", include_str!("../icons/maki/town.svg")),
    ("town-hall", include_str!("../icons/maki/town-hall.svg")),
    ("triangle", include_str!("../icons/maki/triangle.svg")),
    ("triangle-stroked", include_str!("../icons/maki/triangle-stroked.svg")),
    ("tunnel", include_str!("../icons/maki/tunnel.svg")),
    ("veterinary", include_str!("../icons/maki/veterinary.svg")),
    ("viewpoint", include_str!("../icons/maki/viewpoint.svg")),
    ("village", include_str!("../icons/maki/village.svg")),
    ("volcano", include_str!("../icons/maki/volcano.svg")),
    ("volleyball", include_str!("../icons/maki/volleyball.svg")),
    ("warehouse", include_str!("../icons/maki/warehouse.svg")),
    ("waste-basket", include_str!("../icons/maki/waste-basket.svg")),
    ("watch", include_str!("../icons/maki/watch.svg")),
    ("water", include_str!("../icons/maki/water.svg")),
    ("waterfall", include_str!("../icons/maki/waterfall.svg")),
    ("watermill", include_str!("../icons/maki/watermill.svg")),
    ("wetland", include_str!("../icons/maki/wetland.svg")),
    ("wheelchair", include_str!("../icons/maki/wheelchair.svg")),
    ("windmill", include_str!("../icons/maki/windmill.svg")),
    ("zoo", include_str!("../icons/maki/zoo.svg")),
];

/// Other words for the icons: tag values of OpenStreetMap, such as amenity=doctors, and
/// Swedish names, such as vårdcentral.
pub const ALIASES: &[(&str, &str)] = &[
    ("aerodrome", "airport"),
    ("akutmottagning", "hospital"),
    ("alcohol", "alcohol-shop"),
    ("apartment", "lodging"),
    ("apotek", "pharmacy"),
    ("aquarium", "aquarium"),
    ("archaeological_site", "historic"),
    ("arts_centre", "theatre"),
    ("atm", "bank"),
    ("attraction", "attraction"),
    ("badplats", "swimming"),
    ("bakery", "bakery"),
    ("bank", "bank"),
    ("bar", "bar"),
    ("bbq", "bbq"),
    ("beach", "beach"),
    ("bench", "picnic-site"),
    ("bensinstation", "fuel"),
    ("bibliotek", "library"),
    ("bicycle_parking", "bicycle"),
    ("bicycle_rental", "bicycle-share"),
    ("bio", "cinema"),
    ("brandstation", "fire-station"),
    ("bus_station", "bus"),
    ("bus_stop", "bus"),
    ("busshållplats", "bus"),
    ("busstation", "bus"),
    ("cafe", "cafe"),
    ("café", "cafe"),
    ("camp_site", "campsite"),
    ("camping", "campsite"),
    ("car_rental", "car-rental"),
    ("car_repair", "car-repair"),
    ("caravan_site", "campsite"),
    ("casino", "casino"),
    ("castle", "castle"),
    ("cemetery", "cemetery"),
    ("charging_station", "charging-station"),
    ("chemist", "pharmacy"),
    ("church", "religious-christian"),
    ("cinema", "cinema"),
    ("clinic", "doctor"),
    ("clothes", "clothing-store"),
    ("college", "college"),
    ("community_centre", "town-hall"),
    ("convenience", "convenience"),
    ("courthouse", "town-hall"),
    ("cykelparkering", "bicycle"),
    ("defibrillator", "defibrillator"),
    ("dentist", "dentist"),
    ("doctors", "doctor"),
    ("dricksvatten", "drinking-water"),
    ("drinking_water", "drinking-water"),
    ("drivmedel", "fuel"),
    ("embassy", "embassy"),
    ("emergency_phone", "emergency-phone"),
    ("fast_food", "fast-food"),
    ("ferry_terminal", "ferry"),
    ("fire_station", "fire-station"),
    ("fitness_centre", "fitness-centre"),
    ("florist", "florist"),
    ("flygplats", "airport"),
    ("folktandvården", "dentist"),
    ("fuel", "fuel"),
    ("furniture", "furniture"),
    ("fyr", "lighthouse"),
    ("fängelse", "prison"),
    ("färja", "ferry"),
    ("förskola", "school"),
    ("gallery", "art-gallery"),
    ("garden", "garden"),
    ("garden_centre", "garden-centre"),
    ("gift", "gift"),
    ("golf_course", "golf"),
    ("grave_yard", "cemetery"),
    ("guest_house", "lodging"),
    ("gymnasium", "school"),
    ("hairdresser", "hairdresser"),
    ("hardware", "hardware"),
    ("helipad", "heliport"),
    ("hjärtstartare", "defibrillator"),
    ("hospital", "hospital"),
    ("hostel", "lodging"),
    ("hotel", "lodging"),
    ("hotell", "lodging"),
    ("hållplats", "bus"),
    ("högskola", "college"),
    ("ice_cream", "ice-cream"),
    ("ice_rink", "skiing"),
    ("idrottsplats", "pitch"),
    ("information", "information"),
    ("jewelry", "jewelry-store"),
    ("järnvägsstation", "rail"),
    ("kafé", "cafe"),
    ("kindergarten", "school"),
    ("kommunhus", "town-hall"),
    ("kyrka", "religious-christian"),
    ("kyrkogård", "cemetery"),
    ("laddplats", "charging-station"),
    ("laddstation", "charging-station"),
    ("laundry", "laundry"),
    ("lekplats", "playground"),
    ("library", "library"),
    ("lighthouse", "lighthouse"),
    ("livsmedel", "grocery"),
    ("läkare", "doctor"),
    ("marketplace", "shop"),
    ("matbutik", "grocery"),
    ("memorial", "monument"),
    ("monument", "monument"),
    ("moské", "religious-muslim"),
    ("mosque", "religious-muslim"),
    ("motel", "lodging"),
    ("museum", "museum"),
    ("nightclub", "nightclub"),
    ("nursing_home", "hospital"),
    ("optician", "optician"),
    ("park", "park"),
    ("parkering", "parking"),
    ("parking", "parking"),
    ("parking_space", "parking"),
    ("peak", "mountain"),
    ("pharmacy", "pharmacy"),
    ("picnic_site", "picnic-site"),
    ("pitch", "pitch"),
    ("place_of_worship", "place-of-worship"),
    ("playground", "playground"),
    ("police", "police"),
    ("polis", "police"),
    ("polisstation", "police"),
    ("post", "post"),
    ("post_box", "post"),
    ("post_office", "post"),
    ("prison", "prison"),
    ("pub", "beer"),
    ("recycling", "recycling"),
    ("restaurang", "restaurant"),
    ("restaurant", "restaurant"),
    ("ruins", "historic"),
    ("räddningstjänst", "fire-station"),
    ("school", "school"),
    ("shelter", "shelter"),
    ("shoes", "shoe"),
    ("simhall", "swimming"),
    ("sjukhus", "hospital"),
    ("skola", "school"),
    ("social_facility", "home"),
    ("sports_centre", "fitness-centre"),
    ("stadium", "stadium"),
    ("stadshus", "town-hall"),
    ("station", "rail"),
    ("subway_entrance", "rail-metro"),
    ("supermarket", "grocery"),
    ("swimming_pool", "swimming"),
    ("synagoga", "religious-jewish"),
    ("synagogue", "religious-jewish"),
    ("tandläkare", "dentist"),
    ("tandvård", "dentist"),
    ("taxi", "taxi"),
    ("teater", "theatre"),
    ("telephone", "telephone"),
    ("theatre", "theatre"),
    ("theme_park", "amusement-park"),
    ("toalett", "toilet"),
    ("toilets", "toilet"),
    ("townhall", "town-hall"),
    ("tram_stop", "rail-light"),
    ("turistinformation", "information"),
    ("tågstation", "rail"),
    ("universitet", "college"),
    ("university", "college"),
    ("utsiktsplats", "viewpoint"),
    ("vandrarhem", "lodging"),
    ("veterinary", "veterinary"),
    ("veterinär", "veterinary"),
    ("viewpoint", "viewpoint"),
    ("vindskydd", "shelter"),
    ("volcano", "volcano"),
    ("vårdcentral", "doctor"),
    ("waste_basket", "waste-basket"),
    ("waterfall", "waterfall"),
    ("watermill", "watermill"),
    ("wetland", "wetland"),
    ("windmill", "windmill"),
    ("zoo", "zoo"),
    ("äldreboende", "home"),
    ("återvinning", "recycling"),
    ("återvinningscentral", "recycling"),
];

/// The icon a word leads to: the name of an icon, or a word for one, in any case and with
/// spaces, - or _ alike. None when there is none.
pub fn icon_for(word: &str) -> Option<&'static str> {
    let w = word.trim().to_lowercase();
    if w.is_empty() {
        return None;
    }
    let dashed = w.replace([' ', '_'], "-");
    if let Some((n, _)) = MAKI.iter().find(|(n, _)| *n == dashed) {
        return Some(n);
    }
    let under = w.replace([' ', '-'], "_");
    ALIASES.iter().find(|(a, _)| *a == under || *a == w).map(|(_, n)| *n)
}

/// An icon drawn at a size in pixels, as a mask of how much each pixel is covered. Kept, as
/// the same icons are drawn again and again.
pub fn mask(name: &str, px: u32) -> Option<Arc<Pixmap>> {
    type Drawn = HashMap<(String, u32), Option<Arc<Pixmap>>>;
    static CACHE: OnceLock<Mutex<Drawn>> = OnceLock::new();
    let px = px.clamp(4, 512);
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(m) = cache.lock().ok()?.get(&(name.to_string(), px)) {
        return m.clone();
    }
    let made = render(name, px);
    if let Ok(mut c) = cache.lock() {
        c.insert((name.to_string(), px), made.clone());
    }
    made
}

fn render(name: &str, px: u32) -> Option<Arc<Pixmap>> {
    let svg = MAKI.iter().find(|(n, _)| *n == name)?.1;
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).ok()?;
    let mut pm = Pixmap::new(px, px)?;
    let k = px as f32 / tree.size().width().max(tree.size().height());
    resvg::render(&tree, tiny_skia::Transform::from_scale(k, k), &mut pm.as_mut());
    Some(Arc::new(pm))
}

/// Draws an icon with its middle at (x, y), `size` pixels across, in a colour, laid over
/// what is there. With `halo`, a thin edge of that colour goes around it first.
pub fn draw(pm: &mut Pixmap, name: &str, x: f32, y: f32, size: f32, colour: [u8; 4], halo: Option<[u8; 4]>) {
    let px = size.round().max(4.0) as u32;
    let Some(icon) = mask(name, px) else { return };
    let (x0, y0) = ((x - px as f32 / 2.0).round() as i32, (y - px as f32 / 2.0).round() as i32);
    let (pw, ph) = (pm.width() as i32, pm.height() as i32);
    let n = px as i32;
    let cover = |cx: i32, cy: i32| -> f32 {
        if cx < 0 || cy < 0 || cx >= n || cy >= n {
            0.0
        } else {
            icon.pixels()[(cy * n + cx) as usize].alpha() as f32 / 255.0
        }
    };
    let data = pm.data_mut();
    // The picture is premultiplied: a colour is laid over what is there by how much the
    // icon covers the pixel.
    let mut lay = |tx: i32, ty: i32, c: [u8; 4], a: f32| {
        if tx < 0 || ty < 0 || tx >= pw || ty >= ph || a <= 0.0 {
            return;
        }
        let a = a.min(1.0) * c[3] as f32 / 255.0;
        let k = ((ty * pw + tx) * 4) as usize;
        for i in 0..3 {
            data[k + i] = (c[i] as f32 * a + data[k + i] as f32 * (1.0 - a)).round() as u8;
        }
        data[k + 3] = (255.0 * a + data[k + 3] as f32 * (1.0 - a)).round() as u8;
    };
    for cy in -1..=n {
        for cx in -1..=n {
            let own = cover(cx, cy);
            if let Some(h) = halo {
                // The edge: the most the icon covers next to this pixel, where it does not
                // cover the pixel itself.
                let mut near = 0.0f32;
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
                    near = near.max(cover(cx + dx, cy + dy));
                }
                lay(x0 + cx, y0 + cy, h, near * (1.0 - own) + own);
            }
            lay(x0 + cx, y0 + cy, colour, own);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_lead_to_icons_and_they_are_drawn() {
        assert_eq!(icon_for("Hospital"), Some("hospital"));
        assert_eq!(icon_for("charging_station"), Some("charging-station"));
        assert_eq!(icon_for("doctors"), Some("doctor"));
        assert_eq!(icon_for("Vårdcentral"), Some("doctor"));
        assert_eq!(icon_for("hotel"), Some("lodging"));
        assert_eq!(icon_for("police station"), None);
        assert_eq!(icon_for(""), None);
        // Every icon can be read and drawn, and covers part of its square.
        for (name, _) in MAKI {
            let m = mask(name, 30).unwrap_or_else(|| panic!("{} cannot be drawn", name));
            let covered = m.pixels().iter().filter(|p| p.alpha() > 128).count();
            assert!(covered > 20 && covered < 30 * 30, "{}: {}", name, covered);
        }
        let mut pm = Pixmap::new(40, 40).unwrap();
        draw(&mut pm, "hospital", 20.0, 20.0, 30.0, [200, 0, 0, 255], Some([255, 255, 255, 255]));
        let px = |x: u32, y: u32| pm.pixel(x, y).unwrap().demultiply();
        // The cross of the hospital is red in its middle, and nothing far out.
        assert!(px(20, 20).red() > 150 && px(20, 20).green() < 60, "{:?}", px(20, 20));
        assert_eq!(px(1, 1).alpha(), 0);
    }
}
