//! Who the pets are, kept between runs in `/data/pets.json`: each one's
//! kind, name, where it was, what it's learned and what it wears. A 1.0
//! file (kind, name, column) reads as pets just adopted.

use serde_json::{json, Value};

use crate::brain::Pet;
use crate::care::Care;
use crate::world::{Floor, Spot};

pub fn save(pets: &[Pet]) -> String {
    let list: Vec<Value> = pets
        .iter()
        .map(|p| {
            json!({
                "uid": p.uid,
                "kind": p.kind,
                "name": p.name,
                "x": p.to.x,
                "xp": p.care.xp,
                "happy": p.care.happy,
                "adopted": p.care.adopted,
                "wearing": p.wearing,
            })
        })
        .collect();
    serde_json::to_string_pretty(&json!({ "version": 2, "pets": list })).unwrap_or_default()
}

/// The pets in `text`, on the modeline; `today` for any that don't say
/// when they came.
pub fn load(text: &str, today: u32) -> Vec<Pet> {
    let Ok(doc) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    let list = match &doc {
        Value::Array(list) => list.clone(),
        Value::Object(o) => o.get("pets").and_then(Value::as_array).cloned().unwrap_or_default(),
        _ => Vec::new(),
    };
    let mut out: Vec<Pet> = Vec::new();
    for (n, p) in list.iter().enumerate() {
        let (Some(kind), Some(name)) = (p["kind"].as_str(), p["name"].as_str()) else { continue };
        let uid = p["uid"].as_u64().map(|u| u as u32).filter(|u| *u > 0 && !out.iter().any(|o| o.uid == *u)).unwrap_or(1000 + n as u32);
        let x = p["x"].as_f64().unwrap_or(10.0) as f32;
        let mut pet = Pet::new(uid, kind, name, Spot::new(Floor::Modeline, x), today);
        pet.care = Care {
            xp: p["xp"].as_u64().unwrap_or(0) as u32,
            happy: p["happy"].as_f64().map(|h| h as f32).unwrap_or(80.0).clamp(crate::care::CONTENT, 100.0),
            adopted: p["adopted"].as_u64().map(|d| d as u32).unwrap_or(today),
        };
        pet.wearing = p["wearing"].as_array().map(|w| w.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
        out.push(pet);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pets_are_kept_and_read_back() {
        let mut pet = Pet::new(3, "cat", "Tom", Spot::new(Floor::Modeline, 12.5), 50);
        pet.care.xp = 140;
        pet.wearing = vec!["scarf".into()];
        let back = load(&save(&[pet]), 99);
        assert_eq!(back.len(), 1);
        let p = &back[0];
        assert_eq!((p.uid, p.kind.as_str(), p.name.as_str(), p.to.x, p.care.xp, p.care.adopted), (3, "cat", "Tom", 12.5, 140, 50));
        assert_eq!(p.wearing, ["scarf"]);
    }

    #[test]
    fn a_one_point_oh_file_reads_as_new_pets() {
        let old = r#"[{"kind": "ember", "name": "Ember", "x": 12.0}, {"kind": "duck", "name": "Q", "x": 30.0}]"#;
        let pets = load(old, 77);
        assert_eq!(pets.iter().map(|p| (p.name.as_str(), p.care.adopted, p.uid)).collect::<Vec<_>>(), [("Ember", 77, 1000), ("Q", 77, 1001)]);
        assert!(load("not json", 0).is_empty());
    }
}
