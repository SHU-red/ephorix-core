//! Monochrome red-on-black SVG icon system (Spartan theme).
//! All glyphs are inline SVG strings — crisp at any DPI, zero image assets.
//! Palette is strictly #E53935 on black (transparent backgrounds).

/// Agoge type glyph keys offered in the editor.
pub const GLYPH_KEYS: [&str; 10] = [
    "shield",
    "wheel",
    "mountain",
    "winged-foot",
    "oar",
    "wave",
    "rings",
    "ball",
    "glove",
    "lambda",
];

/// SVG for a glyph key; unknown keys fall back to the lambda mark.
pub fn glyph_svg(key: &str) -> &'static str {
    match key {
        "shield" => SHIELD,
        "wheel" => WHEEL,
        "mountain" => MOUNTAIN,
        "winged-foot" => WINGED_FOOT,
        "oar" => OAR,
        "wave" => WAVE,
        "rings" => RINGS,
        "ball" => BALL,
        "glove" => GLOVE,
        _ => LAMBDA,
    }
}

/// Maps a stored icon string (glyph key, legacy text name, or legacy emoji)
/// to a glyph key. Unknown values -> lambda. Never panics, never breaks.
pub fn glyph_key(icon: &str) -> &'static str {
    match icon {
        "dumbbell" | "🏋️" | "🏋" => "shield",
        "bicycle" | "🚴" | "🚴️" => "wheel",
        "mountain" | "🧗" => "mountain",
        "runner" | "🏃" => "winged-foot",
        "rowing" | "🚣" => "oar",
        "swim" | "swimming" | "🏊" => "wave",
        "gymnastics" | "🤸" => "rings",
        "basketball" | "⛹️" | "⛹" => "ball",
        "boxing" | "🥊" => "glove",
        other => {
            for k in GLYPH_KEYS {
                if k == other {
                    return k;
                }
            }
            "lambda"
        }
    }
}

/// The primary mark: the Lakedaimonian lambda.
pub const LAMBDA: &str = concat!(
    r##"<svg viewBox="0 0 24 24" fill="none" stroke="#E53935" stroke-width="2.6" stroke-linecap="square">"##,
    r##"<path d="M4 4 L12 19 L20 4"/>"##,
    r##"</svg>"##,
);

/// Plumed Corinthian helmet — the header hero mark.
pub const HELMET: &str = concat!(
    r##"<svg viewBox="0 0 40 32" fill="none" stroke="#E53935" stroke-width="2" stroke-linecap="square" stroke-linejoin="round">"##,
    // crest (filled plume)
    r##"<path d="M11 11 C 14 3, 26 3, 29 11 C 26 8, 14 8, 11 11 Z" fill="#E53935" stroke="none"/>"##,
    // dome
    r##"<path d="M8 17 C 8 9, 32 9, 32 17"/>"##,
    // cheek guards
    r##"<path d="M8 17 L5 26"/>"##,
    r##"<path d="M32 17 L35 26"/>"##,
    // nose guard
    r##"<path d="M20 9 L20 23"/>"##,
    // eye slits
    r##"<path d="M12 14 L17 14"/>"##,
    r##"<path d="M23 14 L28 14"/>"##,
    r##"</svg>"##,
);

/// Aspis (round Spartan shield).
pub const SHIELD: &str = concat!(
    r##"<svg viewBox="0 0 24 24" fill="none" stroke="#E53935" stroke-width="1.8" stroke-linejoin="round">"##,
    r##"<path d="M12 2.5 C 18 4.5, 21.5 9.5, 21.5 13.5 C 21.5 18.5, 17.5 22, 12 23 C 6.5 22, 2.5 18.5, 2.5 13.5 C 2.5 9.5, 6 4.5, 12 2.5 Z"/>"##,
    r##"</svg>"##,
);

/// Six-spoke Spartan wheel.
pub const WHEEL: &str = concat!(
    r##"<svg viewBox="0 0 24 24" fill="none" stroke="#E53935" stroke-width="1.8" stroke-linecap="square">"##,
    r##"<circle cx="12" cy="12" r="8"/>"##,
    r##"<path d="M12 12 L12 4 M12 12 L18.93 8 M12 12 L18.93 16 M12 12 L12 20 M12 12 L5.07 16 M12 12 L5.07 8"/>"##,
    r##"</svg>"##,
);

/// Twin peaks.
pub const MOUNTAIN: &str = concat!(
    r##"<svg viewBox="0 0 24 24" fill="none" stroke="#E53935" stroke-width="1.8" stroke-linejoin="round">"##,
    r##"<path d="M3 21 L9.5 8 L13 14.5 L16.5 10 L21 21 Z"/>"##,
    r##"</svg>"##,
);

/// Winged foot (Hermes).
pub const WINGED_FOOT: &str = concat!(
    r##"<svg viewBox="0 0 24 24" fill="none" stroke="#E53935" stroke-width="1.8" stroke-linecap="square">"##,
    // wing feathers
    r##"<path d="M2 8 H8 M4 12 H10 M2 16 H8"/>"##,
    // foot + toes
    r##"<path d="M11 7 V20 M11 20 H8 M11 20 H14"/>"##,
    r##"</svg>"##,
);

/// Rowing oar.
pub const OAR: &str = concat!(
    r##"<svg viewBox="0 0 24 24" fill="none" stroke="#E53935" stroke-width="1.8" stroke-linecap="square">"##,
    // handle
    r##"<path d="M12 3 V19"/>"##,
    // blade
    r##"<path d="M12 14 C 17 14, 18 19, 14.5 21.5 C 12.8 22.6, 12 21.5, 12 19 Z"/>"##,
    r##"</svg>"##,
);

/// Wave.
pub const WAVE: &str = concat!(
    r##"<svg viewBox="0 0 24 24" fill="none" stroke="#E53935" stroke-width="1.8" stroke-linecap="square">"##,
    r##"<path d="M2 12 Q 5 8 8 12 T 14 12 T 20 12"/>"##,
    r##"</svg>"##,
);

/// Gymnastics rings.
pub const RINGS: &str = concat!(
    r##"<svg viewBox="0 0 24 24" fill="none" stroke="#E53935" stroke-width="1.8">"##,
    r##"<circle cx="9" cy="13" r="5"/>"##,
    r##"<circle cx="15" cy="13" r="5"/>"##,
    r##"</svg>"##,
);

/// Basketball.
pub const BALL: &str = concat!(
    r##"<svg viewBox="0 0 24 24" fill="none" stroke="#E53935" stroke-width="1.6">"##,
    r##"<circle cx="12" cy="12" r="8.5"/>"##,
    r##"<path d="M7.2 5.2 Q 12 12 7.2 18.8"/>"##,
    r##"<path d="M16.8 5.2 Q 12 12 16.8 18.8"/>"##,
    r##"<path d="M3.8 10 Q 12 13 20.2 10"/>"##,
    r##"</svg>"##,
);

/// Boxing glove.
pub const GLOVE: &str = concat!(
    r##"<svg viewBox="0 0 24 24" fill="none" stroke="#E53935" stroke-width="1.8" stroke-linejoin="round">"##,
    // fist
    r##"<rect x="6" y="6" width="12" height="11" rx="4"/>"##,
    // thumb
    r##"<path d="M6 11 C 3 11, 3 16, 6 16"/>"##,
    // cuff
    r##"<path d="M8 17 V20 M16 17 V20"/>"##,
    r##"</svg>"##,
);
