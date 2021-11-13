use druid::Color;
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use toml::Value;

#[derive(Clone, Debug, Default)]
pub struct Theme {
    scopes: Vec<String>,
    styles: HashMap<String, Style>,
}

#[derive(Clone, Debug)]
pub struct Style {
    fg: Color,
    bg: Color,
    modifiers: Vec<Modifier>,
}

impl Default for Style {
    fn default() -> Self {
        Style::error()
    }
}

impl Style {
    fn error() -> Style {
        Style {
            fg: Color::WHITE,
            bg: Color::BLACK,
            modifiers: vec![],
        }
    }

    pub fn fg(&self) -> Color {
        if !self.reversed() {
            self.fg.clone()
        } else {
            self.bg.clone()
        }
    }

    pub fn bg(&self) -> Color {
        if !self.reversed() {
            self.bg.clone()
        } else {
            self.fg.clone()
        }
    }

    pub fn bold(&self) -> bool {
        self.modifiers.iter().any(|m| matches!(m, Modifier::BOLD))
    }

    pub fn italic(&self) -> bool {
        self.modifiers.iter().any(|m| matches!(m, Modifier::ITALIC))
    }

    pub fn underline(&self) -> bool {
        self.modifiers
            .iter()
            .any(|m| matches!(m, Modifier::UNDERLINE))
    }

    pub fn reversed(&self) -> bool {
        self.modifiers
            .iter()
            .any(|m| matches!(m, Modifier::REVERSED))
    }
}

#[derive(Clone, Debug)]
pub enum Modifier {
    BOLD,
    UNDERLINE,
    ITALIC,
    REVERSED,
}

impl<'de> Deserialize<'de> for Theme {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut styles = HashMap::new();

        if let Ok(mut colors) = HashMap::<String, Value>::deserialize(deserializer) {
            // TODO: alert user of parsing failures in editor
            let palette = colors
                .remove("palette")
                .map(|value| {
                    ThemePalette::try_from(value).unwrap_or_else(|_| ThemePalette::default())
                })
                .unwrap_or_default();

            styles.reserve(colors.len());
            for (name, style_value) in colors {
                let mut style = Style::default();
                palette.parse_style(&mut style, style_value).unwrap();
                styles.insert(name, style);
            }
        }

        let scopes = styles.keys().map(ToString::to_string).collect();
        Ok(Self { scopes, styles })
    }
}

impl Theme {
    pub fn scope(&self, scope: &str) -> Style {
        self.styles
            .get(scope)
            .map(|s| s.clone())
            .unwrap_or(Style::error())
    }

    #[inline]
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }

    pub fn find_scope_index(&self, scope: &str) -> Option<usize> {
        self.scopes().iter().position(|s| s == scope)
    }
}

struct ThemePalette {
    palette: HashMap<String, Color>,
}

impl Default for ThemePalette {
    fn default() -> Self {
        Self {
            palette: Default::default(),
        }
    }
}

impl ThemePalette {
    pub fn new(palette: HashMap<String, Color>) -> Self {
        let ThemePalette {
            palette: mut default,
        } = ThemePalette::default();

        default.extend(palette);
        Self { palette: default }
    }

    pub fn hex_string_to_rgb(s: &str) -> Result<Color, String> {
        if s.starts_with('#') && s.len() >= 7 {
            if let (Ok(red), Ok(green), Ok(blue)) = (
                u8::from_str_radix(&s[1..3], 16),
                u8::from_str_radix(&s[3..5], 16),
                u8::from_str_radix(&s[5..7], 16),
            ) {
                return Ok(Color::rgb8(red, green, blue));
            }
        }

        Err(format!("Theme: malformed hexcode: {}", s))
    }

    fn parse_value_as_str(value: &Value) -> Result<&str, String> {
        value
            .as_str()
            .ok_or(format!("Theme: unrecognized value: {}", value))
    }

    pub fn parse_color(&self, value: Value) -> Result<Color, String> {
        let value = Self::parse_value_as_str(&value)?;

        self.palette
            .get(value)
            .cloned()
            .ok_or("")
            .or_else(|_| Self::hex_string_to_rgb(value))
    }

    pub fn parse_modifier(value: &Value) -> Option<Modifier> {
        match value.as_str()? {
            "bold" => Some(Modifier::BOLD),
            "italic" => Some(Modifier::ITALIC),
            "underline" => Some(Modifier::UNDERLINE),
            _ => None,
        }
    }

    pub fn parse_style(&self, style: &mut Style, value: Value) -> Result<(), String> {
        if let Value::Table(entries) = value {
            for (name, value) in entries {
                match name.as_str() {
                    "fg" => style.fg = self.parse_color(value)?,
                    "bg" => style.bg = self.parse_color(value)?,
                    "modifiers" => {
                        let modifiers = value
                            .as_array()
                            .ok_or("Theme: modifiers should be an array")?;

                        for modifier in modifiers {
                            if let Some(m) = Self::parse_modifier(modifier) {
                                style.modifiers.push(m)
                            }
                        }
                    }
                    _ => return Err(format!("Theme: invalid style attribute: {}", name)),
                }
            }
        } else {
            style.fg = self.parse_color(value)?;
        }
        Ok(())
    }
}

impl TryFrom<Value> for ThemePalette {
    type Error = String;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        let map = match value {
            Value::Table(entries) => entries,
            _ => return Ok(Self::default()),
        };

        let mut palette = HashMap::with_capacity(map.len());
        for (name, value) in map {
            let value = Self::parse_value_as_str(&value)?;
            let color = Self::hex_string_to_rgb(value)?;
            palette.insert(name, color);
        }

        Ok(Self::new(palette))
    }
}
