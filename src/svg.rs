use super::{Value, Wave};
use std::{fmt, mem};
use vcd;

#[derive(Debug)]
pub struct Svg {
    pub wave: String,
    pub bits: Vec<String>,
}

pub(crate) fn render_svg(wave: &mut Wave, end_time: u64) {
    if wave.values.is_empty() {
        return;
    }

    enum State<'a> {
        Wave(Vec<(u64, bool)>),
        Vec(u64, &'a [vcd::Value]),
        X(u64),
    }

    fn add_undet(parts: &mut Vec<String>, start: u64, end: u64) {
        if start == end {
            return;
        }
        parts.push(format!(
            r#"<rect class="x" rx="1" ry="1" x="{}" y="0" width="{}" height="10"/>"#,
            start,
            end - start
        ));
    }

    fn add_vec(parts: &mut Vec<String>, start: u64, end: u64, value: &[vcd::Value]) {
        parts.push(format!(
            r#"<rect class="vec" rx="1" ry="1" x="{x}" y="0" width="{width}" height="10"/>
<g transform="translate({center} 7)"><text>{text}</text></g>"#,
            x = start,
            width = end - start,
            center = (end + start) / 2,
            text = FmtVec(value),
        ));
    }

    fn add_wave(parts: &mut Vec<String>, items: &[(u64, bool)], end_time: u64) {
        fn wave(b: bool) -> u32 {
            if b {
                0
            } else {
                10
            }
        }
        assert!(!items.is_empty());
        let (first, items) = items.split_first().unwrap();
        let mut prev = first;
        let mut text = format!(r#"<polyline points="{} {},"#, first.0, wave(first.1));
        let mut y = wave(first.1);
        for item in items {
            y = wave(item.1);
            if prev.1 != item.1 {
                text += &format!("{} {},{} {},", item.0, wave(prev.1), item.0, y);
            }
            prev = item;
        }
        text += &format!(r#"{} {}"/>"#, end_time, y);
        parts.push(text)
    }

    fn make_wave(wave: &Wave, end_time: u64, bit: Option<usize>) -> String {
        let mut svg_parts = Vec::new();
        let mut state = State::X(0);
        for &(time, ref point) in &wave.values {
            use vcd::Value::*;
            state = match (state, point) {
                (State::X(x), Value::Scalar(V0)) => {
                    add_undet(&mut svg_parts, x, time);
                    State::Wave(vec![(time, false)])
                }
                (State::X(x), Value::Scalar(V1)) => {
                    add_undet(&mut svg_parts, x, time);
                    State::Wave(vec![(time, true)])
                }
                (State::Wave(mut items), Value::Scalar(V0)) => {
                    items.push((time, false));
                    State::Wave(items)
                }
                (State::Wave(mut items), Value::Scalar(V1)) => {
                    items.push((time, true));
                    State::Wave(items)
                }
                (State::Wave(items), Value::Scalar(X)) | (State::Wave(items), Value::Scalar(Z)) => {
                    add_wave(&mut svg_parts, &items, time);
                    State::X(time)
                }
                (State::Wave(ref mut items), Value::Vector(ref vec)) => match vec[bit.unwrap()] {
                    V0 => {
                        items.push((time, false));
                        State::Wave(mem::replace(items, Vec::new()))
                    }
                    V1 => {
                        items.push((time, true));
                        State::Wave(mem::replace(items, Vec::new()))
                    }
                    Z | X => {
                        add_wave(&mut svg_parts, items, time);
                        State::X(time)
                    }
                },
                (State::X(x), Value::Vector(vec)) => {
                    if let Some(bit) = bit {
                        match vec[bit] {
                            V0 => State::Wave(vec![(time, false)]),
                            V1 => State::Wave(vec![(time, true)]),
                            X | Z => State::X(x),
                        }
                    } else if vec.contains(&X) || vec.contains(&Z) {
                        State::X(x)
                    } else {
                        add_undet(&mut svg_parts, x, time);
                        State::Vec(time, &vec)
                    }
                }
                (State::Vec(x, items), Value::Vector(vec)) => {
                    if vec.contains(&X) || vec.contains(&Z) {
                        add_vec(&mut svg_parts, x, time, items);
                        State::X(time)
                    } else if items != &vec[..] {
                        add_vec(&mut svg_parts, x, time, items);
                        State::Vec(time, &vec)
                    } else {
                        State::Vec(x, &items)
                    }
                }
                (State::Vec(x, items), Value::Scalar(X))
                | (State::Vec(x, items), Value::Scalar(Z)) => {
                    add_vec(&mut svg_parts, x, time, items);
                    State::X(time)
                }
                (state, _) => state,
            }
        }
        match state {
            State::X(x) => add_undet(&mut svg_parts, x, end_time),
            State::Wave(items) => add_wave(&mut svg_parts, &items, end_time),
            State::Vec(x, items) => add_vec(&mut svg_parts, x, end_time, items),
        }
        svg_parts.concat()
    }
    wave.svg = Some(Svg {
        wave: format!(
            r#"<svg class="wave" data-id="{id}" data-size="{size}" width="{width}"><g transform="scale(10 2)">{body}</g></svg>"#,
            size = wave.var.size,
            id = wave.var.code,
            body = make_wave(&wave, end_time, None),
            width=end_time,
        ),
        bits: if wave.var.size == 1 {
            Vec::new()
        } else {
            (0..wave.var.size).map(|bit| {
                format!(r#"<svg class="wave" data-id="{id} {bit}" data-size="1" width="{width}"><g transorm="scale(10 2)">{body}</g></svg>"#,
                        id = wave.var.code,
                        bit = bit,
                        body = make_wave(&wave, end_time, Some(bit as usize)),
                        width=end_time,
                )
            }).collect()
        },
    })
}

struct FmtVec<'a>(&'a [vcd::Value]);
impl<'a> fmt::Display for FmtVec<'a> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let value = self.0.iter().fold(0, |acc, item| {
            use vcd::Value::*;
            acc * 2 + match item {
                X | Z | V0 => 0,
                V1 => 1,
            }
        });
        write!(fmt, "{:#0width$x}", value, width = self.0.len() / 4)
    }
}
