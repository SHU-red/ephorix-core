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

/// Same glyph as [`glyph_svg`] with the theme red (`#E53935`) replaced by a
/// caller-chosen hex `color` — for tinted rendering on non-black surfaces
/// (e.g. the colored workout-strip slots). Malformed colors fall back to the
/// theme red rather than injecting anything into the SVG.
pub fn glyph_svg_tinted(key: &str, color: &str) -> String {
    let c = color.trim().to_ascii_lowercase();
    let valid = (c.len() == 7 || c.len() == 4)
        && c.starts_with('#')
        && c[1..].chars().all(|ch| ch.is_ascii_hexdigit());
    glyph_svg(key).replace("#E53935", if valid { &c } else { "#E53935" })
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

/// The primary brand mark: the vectorized Ephorix logo — a filled
/// Lakedaimonian lambda with epigraphic serif feet and an inner spear shaft
/// (apex at top). Self-contained SVG, no defs/ids.
pub const LAMBDA: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 247 244">"##,
    r##"<g transform="matrix(1,0,0,1,-692,-478)">"##,
    r##"<path d="M729.69925815,674.76162757 C729.69925815,674.76162757 777.4691108695614,540.9222331962694 799.2536995697623,479.88716819 C799.2536995697623,479.88716819 832.1127140837444,479.88716819 832.1127140837444,479.88716819 C844.4865087579027,514.5555004873713 867.6613820269979,579.4858033066571 901.63733466,674.6780776200001 C904.40816799,682.75099429 907.05400133,688.9280776200001 909.57483466,693.2093276200001 C912.09566799,697.4905776200001 914.93420966,700.39161929 918.09045966,701.9124526200001 C921.23629299,703.42286929 925.33004299,704.1780776200001 930.37170966,704.1780776200001 C930.37170966,704.1780776200001 937.55920966,704.1780776200001 937.55920966,704.1780776200001 C937.55920966,704.1780776200001 937.55920966,720.0530776200001 937.55920966,720.0530776200001 C937.55920966,720.0530776200001 845.30920966,720.0530776200001 845.30920966,720.0530776200001 C845.30920966,720.0530776200001 845.30920966,704.1780776200001 845.30920966,704.1780776200001 C845.30920966,704.1780776200001 853.99670966,704.1780776200001 853.99670966,704.1780776200001 C862.31962633,704.1780776200001 868.55920966,702.66766095 872.71545966,699.6468276200001 C876.87170966,696.6155776200001 878.94983466,691.94891095 878.94983466,685.6468276200001 C878.94983466,684.13641095 878.82483466,682.56349429 878.57483466,680.9280776200001 C878.32483466,679.28224429 877.94983466,677.45411929 877.44983466,675.4437026200001 C876.93941799,673.42286929 876.30920966,671.40203595 875.55920966,669.3812026200001 C875.55920966,669.3812026200001 861.05886805,627.73333771 832.05818484,544.43760788 C832.05818484,544.43760788 832.08420948,720.05064725 832.08420948,720.05064725 C832.08420948,720.05064725 799.28420984,720.05550799 799.28420984,720.05550799 C799.28420984,720.05550799 799.25820546,544.5791834400001 799.25820546,544.5791834400001 C770.27099059,627.83622953 755.77738315,669.46475257 755.77738315,669.46475257 C755.02738315,671.4855859 754.39717482,673.50641924 753.88675815,675.52725257 C753.38675815,677.53766924 753.01175815,679.36579424 752.76175815,681.01162757 C752.51175815,682.64704424 752.38675815,684.2199609 752.38675815,685.73037757 C752.38675815,692.0324609 754.46488315,696.69912757 758.62113315,699.73037757 C762.77738315,702.7512109 769.0169664800001,704.26162757 777.33988315,704.26162757 C777.33988315,704.26162757 786.02738315,704.26162757 786.02738315,704.26162757 C786.02738315,704.26162757 786.02738315,720.13662757 786.02738315,720.13662757 C786.02738315,720.13662757 693.77738315,720.13662757 693.77738315,720.13662757 C693.77738315,720.13662757 693.77738315,704.26162757 693.77738315,704.26162757 C693.77738315,704.26162757 700.96488315,704.26162757 700.96488315,704.26162757 C706.00654981,704.26162757 710.10029981,703.50641924 713.24613315,701.99600257 C716.40238315,700.47516924 719.24092481,697.57412757 721.76175815,693.29287757 C724.2825914800001,689.01162757 726.92842481,682.83454424 729.69925815,674.76162757 Z" fill="#E53935"/>"##,
    r##"</g>"##,
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
