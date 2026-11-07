//! Value conversions from Lua to the engine's `#[repr(C)]` types.
//!
//! A Lua table is the natural spelling of a rectangle or a colour, and both the
//! positional and the named form are accepted so scripts read the way the author
//! thinks:
//!
//! ```lua
//! gfx:rect { 10, 10, 40, 20 }                          -- positional
//! gfx:rect { x = 10, y = 10, w = 40, h = 20 }          -- named
//! gfx:clear { r = 12, g = 14, b = 22 }                 -- named, alpha defaults to 255
//! gfx:clear "#0c0e16"                                  -- or a hex string
//! ```
//!
//! Everything has a default, so a missing component is an error only when it has no
//! sensible one: a rectangle needs `w`/`h`, a colour does not need `a`.

use giochi_engine::{Color, Rect};
use mlua::{FromLua, Lua, Table, Value};

/// A `GiochiColor` expressed as a Lua table or a `"#rrggbb"` string.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct LuaColor(pub(crate) Color);

/// A `GiochiRect` expressed as a Lua table.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct LuaRect(pub(crate) Rect);

impl FromLua for LuaColor {
    fn from_lua(value: Value, _lua: &Lua) -> mlua::Result<Self> {
        match value {
            Value::String(text) => parse_hex_color(&text.to_str()?).map(LuaColor),
            Value::Table(table) => color_from_table(&table).map(LuaColor),
            other => Err(mlua::Error::FromLuaConversionError {
                from: other.type_name(),
                to: "GiochiColor".to_owned(),
                message: Some(
                    "expected a table { r, g, b [, a] } or a \"#rrggbb\" string".to_owned(),
                ),
            }),
        }
    }
}

impl FromLua for LuaRect {
    fn from_lua(value: Value, _lua: &Lua) -> mlua::Result<Self> {
        match value {
            Value::Table(table) => rect_from_table(&table).map(LuaRect),
            other => Err(mlua::Error::FromLuaConversionError {
                from: other.type_name(),
                to: "GiochiRect".to_owned(),
                message: Some("expected a table { x, y, w, h }".to_owned()),
            }),
        }
    }
}

fn color_from_table(table: &Table) -> mlua::Result<Color> {
    let r = component(table, 1, "r")?;
    let g = component(table, 2, "g")?;
    let b = component(table, 3, "b")?;
    let a = optional_component(table, 4, "a")?.unwrap_or(255);
    Ok(Color {
        r,
        g,
        b,
        a,
        _reserved: [0; 4],
    })
}

fn rect_from_table(table: &Table) -> mlua::Result<Rect> {
    Ok(Rect {
        x: number(table, 1, "x")?,
        y: number(table, 2, "y")?,
        w: number(table, 3, "w")?,
        h: number(table, 4, "h")?,
        _reserved: [0; 4],
    })
}

/// Reads a 0-255 component, preferring the named key over the positional one.
fn component(table: &Table, index: i32, name: &str) -> mlua::Result<u8> {
    match optional_component(table, index, name)? {
        Some(value) => Ok(value),
        None => Err(mlua::Error::RuntimeError(format!(
            "colour is missing component '{name}'"
        ))),
    }
}

fn optional_component(table: &Table, index: i32, name: &str) -> mlua::Result<Option<u8>> {
    let value: Option<f64> = match table.get::<Option<f64>>(name)? {
        Some(named) => Some(named),
        None => table.get::<Option<f64>>(index)?,
    };
    match value {
        None => Ok(None),
        Some(number) => clamp_component(number).map(Some),
    }
}

/// Clamps and rounds a component, complaining only when it is not a number at all.
fn clamp_component(value: f64) -> mlua::Result<u8> {
    if !value.is_finite() {
        return Err(mlua::Error::RuntimeError(
            "colour components must be finite numbers".into(),
        ));
    }
    Ok(value.round().clamp(0.0, 255.0) as u8)
}

fn number(table: &Table, index: i32, name: &str) -> mlua::Result<f32> {
    let value: Option<f32> = match table.get::<Option<f32>>(name)? {
        Some(named) => Some(named),
        None => table.get::<Option<f32>>(index)?,
    };
    match value {
        Some(number) if number.is_finite() => Ok(number),
        Some(_) => Err(mlua::Error::RuntimeError(format!(
            "rect component '{name}' must be finite"
        ))),
        None => Err(mlua::Error::RuntimeError(format!(
            "rect is missing component '{name}'"
        ))),
    }
}

fn parse_hex_color(text: &str) -> mlua::Result<Color> {
    let digits = text.strip_prefix('#').unwrap_or(text);
    let expanded;
    let digits = match digits.len() {
        3 => {
            expanded = digits.chars().flat_map(|c| [c, c]).collect::<String>();
            expanded.as_str()
        }
        6 | 8 => digits,
        _ => {
            return Err(mlua::Error::RuntimeError(format!(
                "'{text}' is not a colour: use \"#rgb\", \"#rrggbb\" or \"#rrggbbaa\""
            )))
        }
    };
    let byte = |range: std::ops::Range<usize>| -> mlua::Result<u8> {
        u8::from_str_radix(&digits[range], 16)
            .map_err(|_| mlua::Error::RuntimeError(format!("'{text}' is not a hexadecimal colour")))
    };
    Ok(Color {
        r: byte(0..2)?,
        g: byte(2..4)?,
        b: byte(4..6)?,
        a: if digits.len() == 8 { byte(6..8)? } else { 255 },
        _reserved: [0; 4],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colours_are_parsed_in_all_three_forms() {
        assert_eq!(parse_hex_color("#0c0e16").unwrap().r, 12);
        assert_eq!(parse_hex_color("0c0e16").unwrap().a, 255);
        assert_eq!(parse_hex_color("#fff").unwrap().r, 255);
        assert_eq!(parse_hex_color("#11223380").unwrap().a, 0x80);
        assert!(parse_hex_color("#12345").is_err());
        assert!(parse_hex_color("#zzzzzz").is_err());
    }

    #[test]
    fn components_are_clamped_not_rejected() {
        assert_eq!(clamp_component(-5.0).unwrap(), 0);
        assert_eq!(clamp_component(300.0).unwrap(), 255);
        assert_eq!(clamp_component(127.6).unwrap(), 128);
        assert!(clamp_component(f64::NAN).is_err());
    }

    #[test]
    fn a_table_prefers_the_named_key_over_the_positional_one() {
        let lua = Lua::new();
        let table: Table = lua.load("return { 1, 2, 3, r = 200 }").eval().unwrap();
        // Named `r` wins for red; positional 2/3 supply green and blue.
        assert_eq!(color_from_table(&table).unwrap().r, 200);
        assert_eq!(color_from_table(&table).unwrap().g, 2);
        assert_eq!(color_from_table(&table).unwrap().b, 3);
        assert_eq!(color_from_table(&table).unwrap().a, 255);
    }
}
