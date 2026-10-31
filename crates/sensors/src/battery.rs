//! Baterías de periféricos, leídas de la API local de YesCharge o de cualquier programa que publique el mismo formato
//! (`docs/battery-api.md`). Solo se lee: PulseLCD nunca habla con los dispositivos.

use pulse_shared::metrics::Device;
use serde::Deserialize;

use crate::http;

const TIMEOUT_MS: i32 = 1500;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatteryState {
    pub low: Vec<Device>,
    pub full: Vec<Device>,
}

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    devices: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    charge: String,
    #[serde(default)]
    low: bool,
    #[serde(default)]
    full: bool,
}

fn device(kind: &str) -> Option<Device> {
    match kind {
        "headset" => Some(Device::Headset),
        "mouse" => Some(Device::Mouse),
        "keyboard" => Some(Device::Keyboard),
        "gamepad" => Some(Device::Gamepad),
        _ => None,
    }
}

fn add(list: &mut Vec<Device>, device: Device) {
    if !list.contains(&device) {
        list.push(device);
    }
}

fn parse(text: &str) -> Option<BatteryState> {
    let answer: Answer = serde_json::from_str(text).ok()?;
    let mut state = BatteryState::default();
    for entry in answer.devices {
        let Some(device) = device(&entry.kind) else { continue };
        // con el cable ya puesto no hay nada que avisar
        if entry.low && entry.charge != "charging" {
            add(&mut state.low, device);
        } else if entry.full {
            add(&mut state.full, device);
        }
    }
    state.low.sort();
    state.full.sort();
    Some(state)
}

/// Estado de las baterías, o `None` si la fuente no responde.
pub fn fetch(url: &str) -> Option<BatteryState> {
    parse(&http::get(url, TIMEOUT_MS).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devices_are_told_apart_by_their_kind() {
        let text = r#"{"devices":[
            {"name":"MCHOSE L7 Ultra+","kind":"mouse","charge":"discharging","low":true,"full":false},
            {"name":"Arctis Nova 5","kind":"headset","charge":"full","low":false,"full":true}]}"#;
        let s = parse(text).unwrap();
        assert_eq!(s.low, [Device::Mouse]);
        assert_eq!(s.full, [Device::Headset]);
    }

    #[test]
    fn renaming_a_device_does_not_lose_its_icon() {
        let s = parse(r#"{"devices":[{"name":"Mi cosa rara","kind":"keyboard","low":true}]}"#).unwrap();
        assert_eq!(s.low, [Device::Keyboard]);
    }

    #[test]
    fn a_low_device_on_the_cable_needs_no_warning() {
        let s = parse(r#"{"devices":[{"kind":"mouse","charge":"charging","low":true}]}"#).unwrap();
        assert!(s.low.is_empty());
    }

    #[test]
    fn unknown_kinds_are_skipped_and_garbage_is_refused() {
        assert_eq!(parse(r#"{"devices":[{"kind":"other","low":true}]}"#).unwrap(), BatteryState::default());
        assert_eq!(parse("no es json"), None);
    }

    #[test]
    fn two_mice_show_one_plate() {
        let s = parse(r#"{"devices":[{"kind":"mouse","low":true},{"kind":"mouse","low":true}]}"#).unwrap();
        assert_eq!(s.low, [Device::Mouse]);
    }
}
