//! Root application: header, timeline, sessions, types. Black/red Spartan UI.
//! UI preferences (series visibility, range) are persisted in the backend
//! DB (`user_settings`) — the web app needs no second volume.

use leptos::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use gloo_net::http::Request;

use crate::api::*;
use crate::icons::{glyph_svg, GLYPH_KEYS, LAMBDA};
use crate::timeline::{SeriesConfig, TimelineChart};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSettings {
    #[serde(default)]
    series: Option<SeriesConfig>,
    #[serde(default)]
    range_days: Option<i64>,
}

/// Build provenance baked into the image by scripts/publish.sh
/// (frontend/version.json) — shown in the footer when present.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionInfo {
    sha: String,
    #[serde(default)]
    branch: String,
    #[serde(default)]
    built_at: String,
}

/// "2026-08-21T12:34:56Z" -> "2026-08-21 12:34 UTC".
fn fmt_built_at(iso: &str) -> String {
    match iso.split_once('T') {
        Some((date, time)) => format!("{date} {} UTC", &time[..time.len().min(5)]),
        None => iso.to_string(),
    }
}

/// Time-range presets for the timeline buttons (label, days).
const RANGES: &[(i64, &str)] = &[(1, "1D"), (7, "1W"), (30, "1M"), (365, "1Y")];

/// Starting point for the user's own PYTHIA directives
/// (settings.aiProvider.systemPrompt, appended to every oracle prompt by the
/// backend). Editable in the Nomoi tab; RESET TO DEFAULT restores this text.
const DEFAULT_SYSTEM_PROMPT: &str =
    "You are the oracle of EphoriX speaking to a Spartan warrior. Answer in the app's voice: \
     short, direct, decisive — no filler, no flattery. Use metric units (kg, km, kcal, hours, %). \
     Never ask clarifying questions: if a detail is missing, choose a sensible default and state \
     it in one line. Always give concrete [PYTHIA] values — no ranges, no conditionals. When the \
     user describes a meal, estimate its calories and macros yourself. Keep replies under 120 \
     words unless detail is requested.";

/// Which bound of the selected session the chart-click picker is writing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PickField {
    Start,
    End,
}

#[derive(Clone)]
struct LogEntry {
    id: u64,
    ts: f64,
    kind: &'static str,
    msg: String,
}

// --- Pythia oracle (AI chat panel) -----------------------------------------

/// One message in the oracle chat thread.
#[derive(Clone)]
struct OracleMsg {
    id: u64,
    /// "user" or "assistant"
    role: &'static str,
    content: String,
}

/// Which editable control fits a proposal's new value.
#[derive(Clone, Copy, PartialEq)]
enum ProposalInput {
    Number,
    Days,
    Bool,
    Text,
}

/// One pending oracle proposal in review: adjustable draft + accept flag.
#[derive(Clone)]
struct OracleProposalRow {
    key: String,
    label: String,
    /// Formatted current value (muted, struck through in the UI).
    current: String,
    reason: String,
    /// Editable draft of the new value as text (parsed on accept).
    draft: String,
    input: ProposalInput,
    checked: bool,
    /// "measurement" | "meal" when accepting POSTs data instead of settings.
    action: Option<String>,
    /// "weight_kg" | "body_fat_pct" for measurement proposals.
    metric: Option<String>,
    /// Suggested numeric value for action proposals (kcal for meals).
    value: Option<f64>,
}

/// Write `value` at a dotted path ("series.heartRate") inside a settings
/// object, creating intermediate objects as needed. False for empty paths.
fn set_path(root: &mut Value, dotted: &str, value: Value) -> bool {
    let parts: Vec<&str> = dotted.split('.').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return false;
    }
    let mut cur = root;
    for part in &parts[..parts.len() - 1] {
        if !cur.is_object() {
            *cur = Value::Object(serde_json::Map::new());
        }
        let key = (*part).to_string();
        cur = match cur.as_object_mut().unwrap().entry(key) {
            serde_json::map::Entry::Occupied(e) => e.into_mut(),
            serde_json::map::Entry::Vacant(e) => e.insert(Value::Object(serde_json::Map::new())),
        };
    }
    if !cur.is_object() {
        *cur = Value::Object(serde_json::Map::new());
    }
    cur.as_object_mut().unwrap().insert(parts[parts.len() - 1].to_string(), value);
    true
}

/// Compact display of a JSON value for the proposal diff ("—" for null).
fn fmt_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "—".to_string(),
        other => other.to_string(),
    }
}

/// Which editor fits a proposal value (the key wins for rangeDays presets).
fn proposal_input_for(key: &str, proposed: &Value) -> ProposalInput {
    if key.contains("rangeDays") {
        ProposalInput::Days
    } else if proposed.as_bool().is_some() {
        ProposalInput::Bool
    } else if proposed.is_number() {
        ProposalInput::Number
    } else {
        ProposalInput::Text
    }
}

/// A proposal value as its editable draft string.
fn proposal_draft(v: &Value) -> String {
    match v {
        Value::Bool(b) => b.to_string(),
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    Gymnasia,
    Agoges,
    Askesis,
    Syntaxis,
    Leonidas,
    Enomotia,
    Syssitia,
    Rank,
    Anapavsis,
    Nomoi,
    Skopos,
}

impl Tab {
    const ALL: [Tab; 11] = [
        Tab::Gymnasia,
        Tab::Agoges,
        Tab::Askesis,
        Tab::Syntaxis,
        Tab::Leonidas,
        Tab::Enomotia,
        Tab::Syssitia,
        Tab::Rank,
        Tab::Anapavsis,
        Tab::Nomoi,
        Tab::Skopos,
    ];

    fn label(self) -> &'static str {
        match self {
            Tab::Gymnasia => "Gymnasia",
            Tab::Agoges => "Agoges",
            Tab::Askesis => "Askesis",
            Tab::Syntaxis => "Syntaxis",
            Tab::Leonidas => "Leonidas",
            Tab::Enomotia => "Enomotia",
            Tab::Syssitia => "Syssitia",
            Tab::Rank => "Rank",
            Tab::Anapavsis => "Anapavsis",
            Tab::Nomoi => "Nomoi",
            Tab::Skopos => "Skopos",
        }
    }

    fn greek(self) -> &'static str {
        match self {
            Tab::Gymnasia => "γυμνάσια",
            Tab::Agoges => "ἀγωγή",
            Tab::Askesis => "ἄσκησις",
            Tab::Syntaxis => "σύνταξις",
            Tab::Leonidas => "Λεωνίδας",
            Tab::Enomotia => "ἐνωμοτία",
            Tab::Syssitia => "συσσίτια",
            Tab::Rank => "τάξις",
            Tab::Anapavsis => "ἀνάπαυσις",
            Tab::Nomoi => "νόμοι",
            Tab::Skopos => "σκοπός",
        }
    }

    fn ipa(self) -> &'static str {
        match self {
            Tab::Gymnasia => "/ɡym.na.sí.a/",
            Tab::Agoges => "/a.ɡɔː.ɡɛ́ː/",
            Tab::Askesis => "/ás.kɛː.sis/",
            Tab::Syntaxis => "/sýn.tak.sis/",
            Tab::Leonidas => "/le.ɔː.ní.daːs/",
            Tab::Enomotia => "/e.nɔː.mo.tí.a/",
            Tab::Syssitia => "/sys.sí.ti.a/",
            Tab::Rank => "/ták.sis/",
            Tab::Anapavsis => "/a.ná.pau̯.sis/",
            Tab::Nomoi => "/nó.moi/",
            Tab::Skopos => "/sko.pós/",
        }
    }

    fn pos(self) -> &'static str {
        match self {
            Tab::Gymnasia | Tab::Syssitia | Tab::Nomoi => "n. pl.",
            _ => "n.",
        }
    }

    fn definition(self) -> &'static str {
        match self {
            Tab::Gymnasia => "The training grounds — the naked place where the body is forged into a weapon. Your command center: heart, steps, sleep, and the Agoge underway.",
            Tab::Agoges => "The Spartan regimen itself. The categories of labor that turn a boy into a warrior. Define your Agoge types — strength, run, climb, row.",
            Tab::Askesis => "Discipline, exercise, training. The raw numbers are sacred: every beat, step, and calorie is the measure of your askesis.",
            Tab::Syntaxis => "Order, arrangement, the battle line. Your workouts drawn up in rank — detected and recorded — like hoplites in formation.",
            Tab::Leonidas => "The king who held the Hot Gates. Set your goals and train toward Leonidas — the ceiling of a Spartan body. Be like Leonidas.",
            Tab::Enomotia => "The sworn band, the smallest Spartan unit, ~36 men bound by oath. Link accounts, compare, and challenge your sworn brothers.",
            Tab::Syssitia => "The common mess — the daily meal shared by Spartan men. Log your food, water, and supplements; the AI estimates what you ate.",
            Tab::Rank => "Station in the line. The ladder of workout success — Hoplite, Paidonomos, Mothax, Hippeis. Earn your rank through discipline, not birth.",
            Tab::Anapavsis => "Rest, refreshment, recovery. Your DYNAMIS: how much the night restored and how much the day's askesis drained.",
            Tab::Nomoi => "The laws — the customs of Lycurgus. The rules that govern this machine: API, token, AI providers.",
            Tab::Skopos => "The watcher, the sentinel, the mark. Every API call, settings change, and interaction — watched and recorded, like the ephors watch the agoge.",
        }
    }

    fn etymology(self) -> &'static str {
        match self {
            Tab::Gymnasia => "From Greek gymnasion, \"place of exercise\".",
            Tab::Agoges => "From Greek agōgē, \"training, education\".",
            Tab::Askesis => "From Greek askein, \"to exercise, to train\".",
            Tab::Syntaxis => "From Greek syntassein, \"to arrange\".",
            Tab::Leonidas => "From Greek leōn \"lion\" + patronymic -idas.",
            Tab::Enomotia => "From Greek enōmotia, \"sworn brotherhood\".",
            Tab::Syssitia => "From Greek syssition, \"common meal\".",
            Tab::Rank => "From the Spartan military hierarchy.",
            Tab::Anapavsis => "From Greek anapausis, \"rest\".",
            Tab::Nomoi => "From Greek nomos, \"law, custom\".",
            Tab::Skopos => "From Greek skopos, \"watcher, mark, aim\".",
        }
    }

    fn english(self) -> &'static str {
        match self {
            Tab::Gymnasia => "Dashboard",
            Tab::Agoges => "Types",
            Tab::Askesis => "Metrics",
            Tab::Syntaxis => "Workouts",
            Tab::Leonidas => "Targets",
            Tab::Enomotia => "Friends",
            Tab::Syssitia => "Nutrition",
            Tab::Rank => "Ranks",
            Tab::Anapavsis => "Recovery",
            Tab::Nomoi => "Settings",
            Tab::Skopos => "Logs",
        }
    }
}

/// Dictionary-style hero box: the tab title explained like a lexicon entry.
#[component]
fn TabHero(tab: Tab) -> impl IntoView {
    let helmet = (tab == Tab::Leonidas).then_some("/assets/helmet_transparent.png");
    view! {
        <div class="tab-hero">
            <div class="th-body">
                <div class="th-content">
                    <div class="th-head">
                        <span class="th-title">
                            <span class="gr">{tab.label()}</span>
                            <span class="en">{tab.english()}</span>
                        </span>
                        <span class="th-greek">{tab.greek()}</span>
                    </div>
                    <div class="th-meta">
                        <span class="th-pos">{tab.pos()}</span>
                        <span class="th-ipa">{tab.ipa()}</span>
                    </div>
                    <p class="th-def">{tab.definition()}</p>
                    <p class="th-etym">{tab.etymology()}</p>
                </div>
                {helmet.map(|src| view! {
                    <img class="th-hero-img" src=src alt="Spartan helmet" />
                })}
            </div>
        </div>
    }
}

/// Spartan rank ladder by number of completed workouts.
fn rank_for(n: usize) -> (&'static str, &'static str) {
    match n {
        n if n >= 30 => ("Hippeis", "the elite — the king's mounted guard"),
        n if n >= 15 => ("Mothax", "the free-born outsider risen by merit"),
        n if n >= 5 => ("Paidonomos", "the trainer of boys"),
        _ => ("Hoplite", "the heavy infantry — where every Spartan begins"),
    }
}

/// "p10 / p50 / p90" for a baseline triple, "—" when the signal has no data.
fn baseline_triple(p: Option<&BaselinePercentile>) -> String {
    match p {
        Some(t) => format!("{:.0} / {:.0} / {:.0}", t.p10, t.p50, t.p90),
        None => "—".to_string(),
    }
}

/// Local calendar date as "YYYY-MM-DD".
fn local_date_str(d: &js_sys::Date) -> String {
    format!("{:04}-{:02}-{:02}", d.get_full_year(), d.get_month() + 1, d.get_date())
}

/// Today's local date as "YYYY-MM-DD".
fn today_str() -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(js_sys::Date::now()));
    local_date_str(&d)
}

/// Shift a "YYYY-MM-DD" local date by whole days (calendar-aware).
fn shift_day(date: &str, delta: i32) -> String {
    let p: Vec<i32> = date.split('-').filter_map(|s| s.parse().ok()).collect();
    let (y, m, d) = match p.as_slice() {
        [y, m, d] => (*y, *m, *d),
        _ => return today_str(),
    };
    local_date_str(&js_sys::Date::new_with_year_month_day(y as u32, m - 1, d + delta))
}

/// Parse a goal text input to f64 (blank/invalid → fallback).
fn goal_f64(s: &str, fallback: f64) -> f64 {
    s.trim().parse().unwrap_or(fallback)
}

/// Progress-bar width percent, capped at 100 like the other target bars.
fn goal_pct(have: f64, goal: f64) -> f64 {
    if goal <= 0.0 { 0.0 } else { (have / goal * 100.0).min(100.0) }
}

/// Monday 00:00 local of the ISO week containing `now_ms` — the anchor for
/// the weekly intensity-hour total (week starts Monday).
fn week_start_ms(now_ms: f64) -> f64 {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(now_ms));
    // get_day(): 0 = Sunday … 6 = Saturday → days since Monday.
    let since_monday = (d.get_day() as f64 + 6.0) % 7.0;
    d.set_hours(0);
    d.set_minutes(0);
    d.get_time() - since_monday * 86_400_000.0
}

/// One-line label for a logged entry in the Syssitia list.
fn meal_label(m: &NutritionMeal) -> String {
    let label = match m.r#type.as_str() {
        "water" => format!("Water — {:.0} ml", m.amount),
        "meal" => {
            let raw = m.meal_type.clone().unwrap_or_else(|| "Meal".to_string());
            let mut chars = raw.chars();
            let first = chars.next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
            let title = format!("{first}{}", chars.as_str());
            format!("{title} — {:.0} kcal · P{:.0}/C{:.0}/F{:.0} g", m.amount, m.protein, m.carbs, m.fat)
        }
        _ => format!("Food — {:.0} kcal · P{:.0}/C{:.0}/F{:.0} g", m.amount, m.protein, m.carbs, m.fat),
    };
    match &m.note {
        Some(n) if !n.trim().is_empty() => format!("{label} · {n}"),
        _ => label,
    }
}

// ---------------------------------------------------------------------------
// Pebble day-history overlay: daily aggregates (noon-UTC anchors) land in the
// normalized `measurements` store, while the timeline buckets only read
// `raw_health_data`. Merge the day rows in for days with no raw coverage so
// backfilled days (watch off / historical) appear in the steps/kcal charts.
// ---------------------------------------------------------------------------

/// One row from `GET /api/v1/measurements` (long-form normalized store).
#[derive(Debug, Clone, Deserialize)]
struct RawMeasurementRow {
    ts: f64,
    metric: String,
    value: f64,
}

/// Seconds in a server bucket string ("13 minutes", "1 hour", "1 day").
fn bucket_secs(s: &str) -> Option<f64> {
    let mut parts = s.split_whitespace();
    let n: f64 = parts.next()?.parse().ok()?;
    let mult = match parts.next()?.to_ascii_lowercase().as_str() {
        "second" | "seconds" | "sec" | "s" => 1.0,
        "minute" | "minutes" | "min" | "m" => 60.0,
        "hour" | "hours" | "h" => 3600.0,
        "day" | "days" | "d" => 86400.0,
        _ => return None,
    };
    Some(n * mult)
}

/// Day-aggregate rows (steps / active_calories) from the normalized store —
/// the Pebble day-history backfill, plus any other source's daily totals.
async fn fetch_day_aggregates(
    base: &str,
    token: &str,
    from_ms: f64,
    to_ms: f64,
) -> Result<Vec<RawMeasurementRow>, String> {
    let mut out = Vec::new();
    for metric in ["steps", "active_calories"] {
        let v = Request::get(&format!("{base}/api/v1/measurements"))
            .header("X-EphoriX-Token", token)
            .query([
                ("from", iso_from_ms(from_ms).as_str()),
                ("to", iso_from_ms(to_ms).as_str()),
                ("metric", metric),
            ])
            .send()
            .await
            .map_err(|e| format!("request failed: {e}"))?
            .json::<Value>()
            .await
            .map_err(|e| format!("invalid json: {e}"))?;
        if let Ok(rows) = serde_json::from_value::<Vec<RawMeasurementRow>>(v["points"].clone()) {
            out.extend(rows);
        }
    }
    Ok(out)
}

/// Overlay daily totals onto timeline buckets that have NO raw_health_data
/// coverage at all. A day row (noon-UTC anchor, full-day value) is spread
/// proportionally over the buckets overlapping that UTC day, so backfilled
/// days render as a steady rate line instead of a noon spike. Days with any
/// raw data (today's live minutes, partial coverage) are left untouched —
/// merging the full-day total there would double-count. Coverage is decided
/// against the pristine buckets first, then all writes apply, so a bucket
/// straddling midnight never looks "covered" because of an earlier merge.
fn merge_day_aggregates(points: &mut Vec<TimelinePoint>, rows: &[RawMeasurementRow], bucket_secs: f64) {
    const DAY_MS: f64 = 86_400_000.0;
    const NOON_MS: f64 = 43_200_000.0;
    if points.is_empty() || rows.is_empty() || bucket_secs <= 0.0 {
        return;
    }
    let bucket_ms = bucket_secs * 1000.0;
    let domain_start = points[0].ts;
    let domain_end = points[points.len() - 1].ts + bucket_ms;
    let day_start_of = |ts: f64| ((ts - NOON_MS) / DAY_MS).floor() * DAY_MS;

    // Pass 1 — decide what to merge, reading the untouched buckets only.
    struct DayMerge {
        day_start: f64,
        ov_start: f64,
        ov_end: f64,
        total_ov: f64,
    }
    let mut merges: Vec<(usize, DayMerge)> = Vec::new();
    for (idx, r) in rows.iter().enumerate() {
        if !(r.value > 0.0) {
            continue;
        }
        // Only noon-anchored day aggregates ("d" rows at 12:00:00 UTC).
        let day_start = day_start_of(r.ts);
        if (r.ts - day_start - NOON_MS).abs() > 3_600_000.0 {
            continue;
        }
        let day_end = day_start + DAY_MS;
        let ov_start = day_start.max(domain_start);
        let ov_end = day_end.min(domain_end);
        if ov_end <= ov_start {
            continue; // day entirely outside the visible range
        }

        // Skip the day if it has ANY raw coverage: the full-day total would
        // double-count the raw buckets' own share.
        let day_has_raw = points.iter().any(|p| {
            p.ts + bucket_ms > day_start
                && p.ts < day_end
                && (p.heart_rate.is_some() || p.steps.unwrap_or(0) > 0 || p.active_calories.unwrap_or(0.0) > 0.0)
        });
        if day_has_raw {
            continue;
        }

        // Total overlap between the day window (clipped to the visible
        // range) and the buckets — the divisor for the proportional spread.
        let total_ov: f64 = points
            .iter()
            .map(|p| (p.ts + bucket_ms).min(ov_end) - p.ts.max(ov_start))
            .filter(|d| *d > 0.0)
            .sum();
        if total_ov <= 0.0 {
            continue;
        }
        merges.push((
            idx,
            DayMerge {
                day_start,
                ov_start,
                ov_end,
                total_ov,
            },
        ));
    }

    // Pass 2 — apply the decided merges.
    for (idx, m) in merges {
        let r = &rows[idx];
        for p in points.iter_mut() {
            let overlap = (p.ts + bucket_ms).min(m.ov_end) - p.ts.max(m.ov_start);
            if overlap <= 0.0 {
                continue;
            }
            let share = r.value * overlap / m.total_ov;
            if r.metric == "steps" {
                p.steps = Some(p.steps.unwrap_or(0) + share.round() as i64);
            } else if r.metric == "active_calories" {
                p.active_calories = Some(p.active_calories.unwrap_or(0.0) + share);
            }
        }
    }
}

pub fn App() -> impl IntoView {
    let (token, set_token) = create_signal("ephorix-dev-1".to_string());
    // Empty = same origin (served behind the web service, /api proxied to
    // the backend). Dev: run `trunk serve --proxy-backend http://localhost:3000`
    // or type the API URL here.
    let (base, set_base) = create_signal(String::new());
    let (days, set_days) = create_signal(7i64);
    let (types, set_types) = create_signal(Vec::<AgogeType>::new());
    let (sessions, set_sessions) = create_signal(Vec::<AgogeSession>::new());
    let (points, set_points) = create_signal(Vec::<TimelinePoint>::new());
    let (selection, set_selection) = create_signal(None::<(f64, f64)>);
    // Point cursor for the contextual select-mode actions: planted by a clean
    // chart click, cleared whenever a range selection takes over.
    let (point_ts, set_point_ts) = create_signal(None::<f64>);
    let (cursor, set_cursor) = create_signal(None::<f64>);
    let (error, set_error) = create_signal(None::<String>);
    let (selected_type, set_selected_type) = create_signal(None::<String>);
    let (series, set_series) = create_signal(SeriesConfig::default());
    let (settings_loaded, set_settings_loaded) = create_signal(false);
    let clear_sel = create_rw_signal(0u32);
    let reset_zoom = create_rw_signal(0u32);
    // Highlighted range chip (None = nothing highlighted: a manual zoom
    // desyncs the chips from the actual x-domain until a preset is re-picked).
    let active_range = create_rw_signal(None::<i64>);
    let (selected_id, set_selected_id) = create_signal(None::<String>);
    let selected_session = create_rw_signal(None::<AgogeSession>);
    // Graphical start/end picking for the details panel: which bound a chart
    // click should write (None = plain click = create open session).
    let (pick_mode, set_pick_mode) = create_signal(false);
    let (pick_field, set_pick_field) = create_signal(None::<PickField>);
    let chart_id = create_rw_signal(0u32);
    let (nutrition, set_nutrition) = create_signal(Vec::<NutritionEvent>::new());
    let (sleep, set_sleep) = create_signal(Vec::<SleepDay>::new());
    let (detections, set_detections) = create_signal(Vec::<Detection>::new());
    let (zoom_mode, set_zoom_mode) = create_signal(true);
    let (ai_text, set_ai_text) = create_signal(String::new());
    let (ai_base, set_ai_base) = create_signal(String::new());
    let (ai_model, set_ai_model) = create_signal(String::new());
    let (ai_key, set_ai_key) = create_signal(String::new());
    let (ai_sys_prompt, set_ai_sys_prompt) = create_signal(String::new());
    let (body_energy, set_body_energy) = create_signal(None::<BodyEnergyDay>);
    let (battery_series, set_battery_series) = create_signal(Vec::<BatterySeriesPoint>::new());
    let (readiness, set_readiness) = create_signal(None::<Vec<ReadinessDay>>);
    let (readiness_error, set_readiness_error) = create_signal(None::<String>);
    let (readiness_loading, set_readiness_loading) = create_signal(true);
    let (baselines, set_baselines) = create_signal(None::<Baselines>);
    let (log, set_log) = create_signal(Vec::<LogEntry>::new());
    let log_counter = create_rw_signal(0u64);
    // Persisted action log (Skopos): server-logged settings/nutrition/measurement
    // actions with REVERT — distinct from the in-memory event feed above.
    let (actions, set_actions) = create_signal(Vec::<ActionLogEntry>::new());
    let log_event = move |kind: &'static str, msg: &str| {
        let id = log_counter.get();
        log_counter.set(id + 1);
        set_log.update(|l| {
            l.insert(0, LogEntry { id, ts: js_sys::Date::now(), kind, msg: msg.to_string() });
            if l.len() > 300 {
                l.truncate(300);
            }
        });
    };
    let (current_tab, set_current_tab) = create_signal(Tab::Gymnasia);
    // Tap-to-flip for the wordmark: hover doesn't exist on touch devices, so
    // the h1 click handler toggles the `flip` class (styled in style.css).
    let (brand_flip, set_brand_flip) = create_signal(false);
    // Leonidas targets (persisted in settings).
    let (target_steps, set_target_steps) = create_signal(10_000i64);
    let (target_kcal, set_target_kcal) = create_signal(500i64);
    let (target_sleep, set_target_sleep) = create_signal(8.0f64);
    let (target_intensity, set_target_intensity) = create_signal(6.0f64); // intensityHoursPerWeek
    let (target_weight, set_target_weight) = create_signal(82.0f64);      // weightKg
    let (target_body_fat, set_target_body_fat) = create_signal(15.0f64);  // bodyFatPct
    // Leonidas measurements: user-logged weight/body-fat + entry drafts.
    let (measurements, set_measurements) = create_signal(Vec::<Measurement>::new());
    let (meas_weight, set_meas_weight) = create_signal(String::new());
    let (meas_body_fat, set_meas_body_fat) = create_signal(String::new());
    let meas_refresh = create_rw_signal(0u32);
    // Syssitia manual entry.
    let (manual_kind, set_manual_kind) = create_signal("food".to_string());
    let (manual_amount, set_manual_amount) = create_signal(String::new());
    // Nomoi import panel: pick file -> client-side parse -> preview -> POST /import.
    let (import_source, set_import_source) = create_signal("csv".to_string());
    let (import_device, set_import_device) = create_signal(String::new());
    let (import_name, set_import_name) = create_signal(String::new());
    let (import_buf, set_import_buf) = create_signal(Vec::<Value>::new());
    let (import_preview, set_import_preview) = create_signal(Vec::<(String, String, f64)>::new());
    let (import_errors, set_import_errors) = create_signal(Vec::<String>::new());
    let (importing, set_importing) = create_signal(false);
    let (import_result, set_import_result) = create_signal(None::<(usize, usize, Vec<String>)>);
    // Syssitia daily log: selected day, daily totals + entries, goals.
    let (nut_day, set_nut_day) = create_signal(today_str());
    let (nut_daily, set_nut_daily) = create_signal::<Option<NutritionDaily>>(None);
    let (manual_protein, set_manual_protein) = create_signal(String::new());
    let (manual_carbs, set_manual_carbs) = create_signal(String::new());
    let (manual_fat, set_manual_fat) = create_signal(String::new());
    let (manual_meal_type, set_manual_meal_type) = create_signal("breakfast".to_string());
    let (manual_note, set_manual_note) = create_signal(String::new());
    // Nutrition goals — persisted under settings.nutrition.
    let (goal_water, set_goal_water) = create_signal("2500".to_string());
    let (goal_kcal, set_goal_kcal) = create_signal("2200".to_string());
    let (goal_protein, set_goal_protein) = create_signal("140".to_string());
    let (goal_carb, set_goal_carb) = create_signal("250".to_string());
    let (goal_fat, set_goal_fat) = create_signal("70".to_string());
    // Pythia oracle: floating AI chat panel (state digest + thread + proposals).
    let (oracle_open, set_oracle_open) = create_signal(false);
    let (oracle_msgs, set_oracle_msgs) = create_signal(Vec::<OracleMsg>::new());
    let (oracle_busy, set_oracle_busy) = create_signal(false);
    let (oracle_input, set_oracle_input) = create_signal(String::new());
    let (oracle_error, set_oracle_error) = create_signal(None::<String>);
    let (oracle_digest, set_oracle_digest) = create_signal(Vec::<(String, String)>::new());
    let (oracle_proposals, set_oracle_proposals) = create_signal(Vec::<OracleProposalRow>::new());
    let oracle_msg_seq = create_rw_signal(0u64);
    let oracle_thread_ref: NodeRef<leptos::html::Div> = create_node_ref();
    // AI provider config (persisted as settings.aiProvider).
    let (ai_provider, set_ai_provider) = create_signal("llamacpp".to_string());
    let (ai_testing, set_ai_testing) = create_signal(false);
    let (ai_test_result, set_ai_test_result) = create_signal(None::<(bool, String)>);
    // Build provenance footer line, e.g. "SHA a1b2c3d · main · built 2026-08-21 12:34 UTC".
    let (version_line, set_version_line) = create_signal(None::<String>);



    // Persist series visibility + range to the backend settings.
    let persist_settings = move || {
        let base = base.get();
        let token = token.get();
        let s = series.get();
        let d = days.get();
        let ai = json!({ "provider": ai_provider.get(), "baseUrl": ai_base.get(), "model": ai_model.get(), "apiKey": ai_key.get(), "systemPrompt": ai_sys_prompt.get() });
        let targets = json!({
            "steps": target_steps.get(),
            "kcal": target_kcal.get(),
            "sleepH": target_sleep.get(),
            "intensityHoursPerWeek": target_intensity.get(),
            "weightKg": target_weight.get(),
            "bodyFatPct": target_body_fat.get(),
        });
        spawn_local(async move {
            let body = json!({
                "series": s,
                "rangeDays": d,
                "aiProvider": ai,
                "targets": targets,
            });
            let _ = put_settings(&base, &token, &body).await;
            log_event("settings", "saved series/range/aiProvider/targets");
        });
    };

    let refresh = move || {
        set_error.set(None);
        let base = base.get();
        let token = token.get();
        let days = days.get();
        let loaded = settings_loaded.get();
        let to_ms = js_sys::Date::now();
        let from_ms = to_ms - days as f64 * 86_400_000.0;
        let bucket = nice_bucket((to_ms - from_ms) / 1000.0 / 800.0);
        log_event("api", &format!("sync · range {days}d"));

        spawn_local(async move {
            match fetch_timeline(&base, &token, from_ms, to_ms, &bucket).await {
                Ok(mut tt) => {
                    // Pebble day-history backfill: daily aggregates land in
                    // `measurements` at noon-UTC anchors, while the timeline
                    // buckets only read raw_health_data — overlay the day rows
                    // for days with no raw coverage (watch off / historical).
                    if days >= 2 {
                        match fetch_day_aggregates(&base, &token, from_ms, to_ms).await {
                            Ok(rows) => {
                                let b_secs = bucket_secs(&tt.bucket).unwrap_or_else(|| {
                                    if tt.points.len() >= 2 {
                                        (tt.points[tt.points.len() - 1].ts - tt.points[0].ts)
                                            / (tt.points.len() - 1) as f64
                                            / 1000.0
                                    } else {
                                        3600.0
                                    }
                                });
                                merge_day_aggregates(&mut tt.points, &rows, b_secs);
                            }
                            Err(_) => {}
                        }
                    }
                    set_points.set(tt.points);
                    set_sessions.set(tt.sessions);
                    set_nutrition.set(tt.nutrition);
                    set_sleep.set(tt.sleep);
                }
                Err(e) => set_error.set(Some(e)),
            }
            match fetch_types(&base, &token).await {
                Ok(t) => set_types.set(t),
                Err(e) => set_error.set(Some(e)),
            }
            match fetch_workouts(&base, &token, from_ms, to_ms).await {
                Ok(d) => set_detections.set(d),
                Err(e) => set_error.set(Some(e)),
            }
            match fetch_body_battery(&base, &token, from_ms, to_ms).await {
                Ok(b) => set_body_energy.set(b),
                Err(_) => {}
            }
            match fetch_body_battery_series(&base, &token, from_ms, to_ms, &bucket).await {
                Ok(s) => set_battery_series.set(s),
                Err(_) => {}
            }
            // Readiness (today + last 14 days) and 90-day baselines —
            // independent of the main timeline range.
            let r_from = to_ms - 14.0 * 86_400_000.0;
            set_readiness_error.set(None);
            set_readiness_loading.set(true);
            match fetch_readiness(&base, &token, r_from, to_ms).await {
                Ok(d) => set_readiness.set(Some(d)),
                Err(e) => set_readiness_error.set(Some(e)),
            }
            set_readiness_loading.set(false);
            if let Ok(b) = fetch_baselines(&base, &token).await {
                set_baselines.set(Some(b));
            }
            // Settings load once per session (do not clobber user changes).
            if !loaded {
                if let Ok(sv) = fetch_settings(&base, &token).await {
                    if let Ok(stored) = serde_json::from_value::<StoredSettings>(sv.clone()) {
                        if let Some(sc) = stored.series {
                            set_series.set(sc);
                        }
                        if let Some(rd) = stored.range_days {
                            set_days.set(rd);
                        }
                    }
                    if let Some(ai) = sv.get("aiProvider") {
                        set_ai_provider.set(ai.get("provider").and_then(|v| v.as_str()).unwrap_or("llamacpp").to_string());
                        set_ai_base.set(ai.get("baseUrl").and_then(|v| v.as_str()).unwrap_or("").to_string());
                        set_ai_model.set(ai.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string());
                        set_ai_key.set(ai.get("apiKey").and_then(|v| v.as_str()).unwrap_or("").to_string());
                        set_ai_sys_prompt.set(ai.get("systemPrompt").and_then(|v| v.as_str()).unwrap_or("").to_string());
                    }
                    if let Some(t) = sv.get("targets") {
                        set_target_steps.set(t.get("steps").and_then(|v| v.as_i64()).unwrap_or(10_000));
                        set_target_kcal.set(t.get("kcal").and_then(|v| v.as_i64()).unwrap_or(500));
                        set_target_sleep.set(t.get("sleepH").and_then(|v| v.as_f64()).unwrap_or(8.0));
                        set_target_intensity.set(t.get("intensityHoursPerWeek").and_then(|v| v.as_f64()).unwrap_or(6.0));
                        set_target_weight.set(t.get("weightKg").and_then(|v| v.as_f64()).unwrap_or(82.0));
                        set_target_body_fat.set(t.get("bodyFatPct").and_then(|v| v.as_f64()).unwrap_or(15.0));
                    }
                    if let Some(n) = sv.get("nutrition") {
                        if let Some(v) = n.get("waterGoalMl").and_then(|v| v.as_f64()) {
                            set_goal_water.set(v.to_string());
                        }
                        if let Some(v) = n.get("kcalGoal").and_then(|v| v.as_f64()) {
                            set_goal_kcal.set(v.to_string());
                        }
                        if let Some(v) = n.get("proteinGoal").and_then(|v| v.as_f64()) {
                            set_goal_protein.set(v.to_string());
                        }
                        if let Some(v) = n.get("carbGoal").and_then(|v| v.as_f64()) {
                            set_goal_carb.set(v.to_string());
                        }
                        if let Some(v) = n.get("fatGoal").and_then(|v| v.as_f64()) {
                            set_goal_fat.set(v.to_string());
                        }
                    }

                    set_settings_loaded.set(true);
                }
            }
        });
    };

    // Initial load + auto-reload whenever base/token/range changes.
    create_effect(move |_| {
        refresh();
    });

    // Build provenance (one-shot, no auth): fetch /version.json baked by
    // scripts/publish.sh and render the footer line. Graceful: any failure
    // (no file, non-200, unparseable — e.g. plain `trunk serve`) hides it.
    spawn_local(async move {
        let line = async {
            let resp = Request::get("/version.json").send().await.map_err(|e| e.to_string())?;
            if !resp.ok() {
                return Err(format!("version.json: HTTP {}", resp.status()));
            }
            let v: VersionInfo = resp.json().await.map_err(|e| e.to_string())?;
            let mut parts = vec![format!("SHA {}", v.sha)];
            if !v.branch.is_empty() {
                parts.push(v.branch);
            }
            if !v.built_at.is_empty() {
                parts.push(format!("built {}", fmt_built_at(&v.built_at)));
            }
            Ok(parts.join(" · "))
        }
        .await;
        if let Ok(line) = line {
            set_version_line.set(Some(line));
        }
    });

    // -- Timeline actions ----------------------------------------------------

    let create_from_selection = move |_| {
        let Some((from, to)) = selection.get() else {
            set_error.set(Some("drag on the timeline to select a range".to_string()));
            return;
        };
        if (to - from).abs() < 1000.0 {
            set_error.set(Some("selection too short".to_string()));
            return;
        }
        let base = base.get();
        let token = token.get();
        let body = json!({
            "typeId": selected_type.get(),
            "startTime": iso_from_ms(from.min(to)),
            "endTime": iso_from_ms(from.max(to)),
        });
        spawn_local(async move {
            match post_json(&base, &token, "/api/v1/agoge-sessions", &body).await {
                Ok(_) => {
                    clear_sel.set(clear_sel.get() + 1);
                    set_selection.set(None);
                    refresh();
                }
                Err(e) => set_error.set(Some(e)),
            }
        });
    };

    // A range selection and the point cursor are mutually exclusive: once a
    // drag lands a selection, drop any point marker (and vice versa, the
    // click handler clears the selection when planting a point).
    create_effect(move |_| {
        if selection.get().is_some() {
            set_point_ts.set(None);
        }
    });

    // "AGOGE START": begin an open agoge session at the pointed instant,
    // typed by the lower agoge-type bar (selected_type).
    let agoge_start = move |_| {
        let Some(ts) = point_ts.get() else {
            set_error.set(Some("click the timeline to place the start point".to_string()));
            return;
        };
        if sessions.get().into_iter().any(|s| s.status == "active") {
            set_error.set(Some("an agoge is already open — set its end instead".to_string()));
            return;
        }
        let Some(type_id) = selected_type.get() else {
            set_error.set(Some("choose an agoge type below".to_string()));
            return;
        };
        let base = base.get();
        let token = token.get();
        let body = json!({
            "typeId": type_id,
            "startTime": iso_from_ms(ts),
            "endTime": null,
        });
        spawn_local(async move {
            match post_json(&base, &token, "/api/v1/agoge-sessions", &body).await {
                Ok(_) => {
                    set_point_ts.set(None);
                    refresh();
                }
                Err(e) => set_error.set(Some(e)),
            }
        });
    };

    // "AGOGE END": close the user's OPEN agoge session at the pointed instant
    // and post the stop marker (mirrors the removed close-open flow; marker
    // failures are ignored).
    let agoge_end = move |_| {
        let Some(ts) = point_ts.get() else {
            set_error.set(Some("click the timeline to place the end point".to_string()));
            return;
        };
        let Some(open) = sessions.get().into_iter().find(|s| s.status == "active") else {
            set_error.set(Some("no open agoge".to_string()));
            return;
        };
        let id = open.id.clone();
        let end = iso_from_ms(ts);
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            let _ = post_json(
                &base,
                &token,
                "/api/v1/events/marker",
                &json!({
                    "kind": "stop",
                    "sessionId": id.clone(),
                    "occurredAt": end,
                    "source": "web"
                }),
            )
            .await;
            match patch_json(&base, &token, &format!("/api/v1/agoge-sessions/{id}"), &json!({ "endTime": end })).await {
                Ok(_) => {
                    set_point_ts.set(None);
                    refresh();
                }
                Err(e) => set_error.set(Some(e)),
            }
        });
    };

    let set_range_days = move |d: i64| {
        set_days.set(d);
        persist_settings();
    };

    let delete_session = move |id: String| {
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            match delete_json(&base, &token, &format!("/api/v1/agoge-sessions/{id}")).await {
                Ok(_) => refresh(),
                Err(e) => set_error.set(Some(e)),
            }
        });
    };

    // -- Timeline marker interactions ---------------------------------------

    // Cancel graphical time picking (after a pick lands or the selection
    // moves on).
    let end_pick = move || {
        set_pick_field.set(None);
        set_pick_mode.set(false);
    };

    // Chart is ready: remember its bridge id.
    let handle_chart_ready = move |id: u32| {
        chart_id.set(id);
    };

    // Clean left-click on empty plot area: in pick mode it plants the picked
    // start/end on the selected session — an End pick on the open session also
    // posts the stop marker (mirrors the close-open flow). Without a pick
    // active, the click plants the point cursor (AGOGE START/END targets) and
    // drops any range selection; sessions are created via the contextual
    // select-mode actions, never by an accidental chart click.
    let handle_chart_click = move |ts: f64| {
        let Some(field) = pick_field.get() else {
            set_point_ts.set(Some(ts));
            set_selection.set(None);
            return;
        };
        let Some(s) = selected_session.get() else {
            end_pick();
            return;
        };
        let base = base.get();
        let token = token.get();
        let is_end = field == PickField::End;
        let end = iso_from_ms(ts);
        let body = match field {
            PickField::Start => json!({ "startTime": iso_from_ms(ts) }),
            PickField::End => json!({ "endTime": end.clone() }),
        };
        let id = s.id.clone();
        end_pick();
        spawn_local(async move {
            if is_end {
                // Keep the marker event stream complete for retro-analysis
                // (mirrors the removed close_open_at_cursor flow; ignore
                // marker failures).
                let _ = post_json(
                    &base,
                    &token,
                    "/api/v1/events/marker",
                    &json!({
                        "kind": "stop",
                        "sessionId": id.clone(),
                        "occurredAt": end,
                        "source": "web"
                    }),
                )
                .await;
            }
            match patch_json(&base, &token, &format!("/api/v1/agoge-sessions/{id}"), &body).await {
                Ok(_) => {
                    selected_session.set(None);
                    refresh();
                }
                Err(e) => set_error.set(Some(e)),
            }
        });
    };

    // Click on a session bar or chip -> open the details panel for it.
    let handle_session_click = move |sid: String| {
        if selected_session.get().map(|s| s.id) != Some(sid.clone()) {
            end_pick();
        }
        let found = sessions.get().into_iter().find(|s| s.id == sid);
        selected_session.set(found);
    };

    // Type change from the details panel: PATCH just the typeId.
    let save_session_type = move |id: String, type_id: String| {
        let tid: Option<String> = if type_id.is_empty() { None } else { Some(type_id) };
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            match patch_json(&base, &token, &format!("/api/v1/agoge-sessions/{id}"), &json!({ "typeId": tid })).await {
                Ok(_) => refresh(),
                Err(e) => set_error.set(Some(e)),
            }
        });
    };

    let close_selected_session = move |id: String| {
        let base = base.get();
        let token = token.get();
        let end = iso_from_ms(js_sys::Date::now());
        spawn_local(async move {
            match patch_json(&base, &token, &format!("/api/v1/agoge-sessions/{id}"), &json!({ "endTime": end })).await {
                Ok(_) => {
                    selected_session.set(None);
                    end_pick();
                    refresh();
                }
                Err(e) => set_error.set(Some(e)),
            }
        });
    };

    let delete_selected_session = move |id: String| {
        selected_session.set(None);
        end_pick();
        delete_session(id);
    };

    // Toggle graphical picking of one bound; a second click on the same
    // button cancels the pick.
    let toggle_pick = move |field: PickField| {
        if pick_field.get() == Some(field) {
            set_pick_field.set(None);
            set_pick_mode.set(false);
        } else {
            set_pick_field.set(Some(field));
            set_pick_mode.set(true);
        }
    };

    // "DURATION" override from the details panel: end = start + HH:MM.
    let apply_duration = move |id: String, duration: String| {
        let Some(s) = sessions.get().into_iter().find(|s| s.id == id) else {
            return;
        };
        let Some(start_ms) = ms_from_iso(&s.start_time) else {
            set_error.set(Some("session has no start time".to_string()));
            return;
        };
        let Some(secs) = parse_hhmm(&duration) else {
            set_error.set(Some("duration must be HH:MM".to_string()));
            return;
        };
        let end = iso_from_ms(start_ms + secs as f64 * 1000.0);
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            match patch_json(&base, &token, &format!("/api/v1/agoge-sessions/{id}"), &json!({ "endTime": end })).await {
                Ok(_) => {
                    selected_session.set(None);
                    refresh();
                }
                Err(e) => set_error.set(Some(e)),
            }
        });
    };


    // -- Agoge types CRUD ----------------------------------------------------

    let create_type = move |name: String, color: String, icon: String, category: String, config: Value| {
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            let body = json!({ "name": name, "colorCode": color, "icon": icon, "category": category, "config": config });
            match post_json(&base, &token, "/api/v1/agoge-types", &body).await {
                Ok(_) => { log_event("agoge", "created agoge"); refresh(); }
                Err(e) => set_error.set(Some(e)),
            }
        });
    };

    let update_type = move |id: String, name: String, color: String, icon: String, category: String, config: Value| {
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            let body = json!({ "name": name, "colorCode": color, "icon": icon, "category": category, "config": config });
            match put_json(&base, &token, &format!("/api/v1/agoge-types/{id}"), &body).await {
                Ok(_) => {
                    log_event("agoge", "updated agoge");
                    refresh();
                }
                Err(e) => set_error.set(Some(e)),
            }
        });
    };

    let delete_type = move |id: String| {
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            match delete_json(&base, &token, &format!("/api/v1/agoge-types/{id}")).await {
                Ok(_) => { log_event("agoge", "deleted agoge"); refresh(); }
                Err(e) => set_error.set(Some(e)),
            }
        });
    };

    let accept_det = move |id: String| {
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            match accept_detection(&base, &token, &id).await {
                Ok(_) => { log_event("detection", "accepted workout"); refresh(); }
                Err(e) => set_error.set(Some(e)),
            }
        });
    };

    let reject_det = move |id: String| {
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            match reject_detection(&base, &token, &id).await {
                Ok(_) => { log_event("detection", "rejected workout"); refresh(); }
                Err(e) => set_error.set(Some(e)),
            }
        });
    };

    // Syssitia daily log: fetch the selected day's totals + entries whenever
    // the day (or api base/token) changes, or after a manual bump.
    let nut_refresh = create_rw_signal(0u32);
    create_effect(move |_| {
        let _ = nut_refresh.get();
        let day = nut_day.get();
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            match fetch_daily_nutrition(&base, &token, &day).await {
                Ok(d) => set_nut_daily.set(Some(d)),
                Err(e) => set_error.set(Some(e)),
            }
        });
    });
    let nut_day_nav = move |delta: i32| set_nut_day.update(|d| *d = shift_day(d, delta));
    let nut_day_today = move |_| set_nut_day.set(today_str());

    // AI-assisted nutrition entry: describe a meal/drink -> parse -> log.
    let ai_submit = move |_| {
        let text = ai_text.get().trim().to_string();
        if text.is_empty() {
            return;
        }
        let base = base.get();
        let token = token.get();
        set_error.set(None);
        spawn_local(async move {
            match parse_ai(&base, &token, &text).await {
                Ok(p) => {
                    let kind = p.get("kind").and_then(|v| v.as_str()).unwrap_or("food");
                    let amount = p.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let body = json!({
                        "kind": kind,
                        "amount": amount,
                        "consumedAt": iso_from_ms(js_sys::Date::now()),
                        "note": text,
                    });
                    if let Err(e) = add_nutrition(&base, &token, &body).await {
                        set_error.set(Some(e));
                    }
                    set_ai_text.set(String::new());
                    log_event("ai", &format!("logged nutrition via AI ({kind} {amount:.0})"));
                    refresh();
                    nut_refresh.set(nut_refresh.get() + 1);
                }
                Err(e) => set_error.set(Some(e)),
            }
        });
    };

    let add_nutrition_manual = move |_| {
        let kind = manual_kind.get();
        let amount: f64 = manual_amount.get().trim().parse().unwrap_or(0.0);
        if amount <= 0.0 {
            return;
        }
        let protein = manual_protein.get().trim().parse::<f64>().unwrap_or(0.0);
        let carbs = manual_carbs.get().trim().parse::<f64>().unwrap_or(0.0);
        let fat = manual_fat.get().trim().parse::<f64>().unwrap_or(0.0);
        if protein < 0.0 || carbs < 0.0 || fat < 0.0 {
            return;
        }
        // The backend stores every entry as water|food; a meal is a food
        // entry tagged with its meal type.
        let meal_type = if kind == "meal" { Some(manual_meal_type.get()) } else { None };
        let note = manual_note.get().trim().to_string();
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            let mut body = json!({
                "kind": if kind == "water" { "water" } else { "food" },
                "amount": amount,
                "consumedAt": iso_from_ms(js_sys::Date::now()),
            });
            if kind != "water" {
                body["protein"] = json!(protein);
                body["carbs"] = json!(carbs);
                body["fat"] = json!(fat);
            }
            if let Some(mt) = meal_type.filter(|mt| !mt.is_empty()) {
                body["mealType"] = json!(mt);
            }
            if !note.is_empty() {
                body["note"] = json!(note);
            }
            if let Err(e) = add_nutrition(&base, &token, &body).await {
                set_error.set(Some(e));
                return;
            }
            set_manual_amount.set(String::new());
            set_manual_protein.set(String::new());
            set_manual_carbs.set(String::new());
            set_manual_fat.set(String::new());
            set_manual_note.set(String::new());
            log_event("nutrition", &format!("logged {kind} {amount:.0}"));
            refresh();
            nut_refresh.set(nut_refresh.get() + 1);
        });
    };

    // Persist nutrition goals into settings.nutrition, merging with the
    // existing settings blob so other keys are preserved.
    let save_nutrition_goals = move |_| {
        let base = base.get();
        let token = token.get();
        let goals = json!({
            "waterGoalMl": goal_water.get().trim().parse::<f64>().unwrap_or(2500.0),
            "kcalGoal": goal_kcal.get().trim().parse::<f64>().unwrap_or(2200.0),
            "proteinGoal": goal_protein.get().trim().parse::<f64>().unwrap_or(140.0),
            "carbGoal": goal_carb.get().trim().parse::<f64>().unwrap_or(250.0),
            "fatGoal": goal_fat.get().trim().parse::<f64>().unwrap_or(70.0),
        });
        spawn_local(async move {
            let mut settings = fetch_settings(&base, &token).await.unwrap_or(Value::Null);
            if !settings.is_object() {
                settings = json!({});
            }
            settings["nutrition"] = goals;
            if let Err(e) = put_settings(&base, &token, &settings).await {
                set_error.set(Some(e));
            } else {
                log_event("settings", "saved nutrition goals");
                nut_refresh.set(nut_refresh.get() + 1);
            }
        });
    };

    // -- Leonidas measurements + persisted action log -------------------------

    // Weekly intensity hours from CLOSED agoge sessions whose start falls in
    // the current ISO week (duration_sec comes from the watch stop summary).
    let week_intensity_h = move || {
        let ws = week_start_ms(js_sys::Date::now());
        let we = ws + 7.0 * 86_400_000.0;
        let mut secs = 0i64;
        for s in sessions.get() {
            if s.status == "active" {
                continue;
            }
            if let Some(start) = ms_from_iso(&s.start_time) {
                if start >= ws && start < we {
                    secs += s.duration_sec.unwrap_or(0);
                }
            }
        }
        secs as f64 / 3600.0
    };

    // Recent weight/body-fat measurements (newest first), refetched after
    // every measurement POST (and whenever base/token change).
    create_effect(move |_| {
        let _ = meas_refresh.get();
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            let mut rows = Vec::new();
            for metric in ["weight_kg", "body_fat_pct"] {
                match fetch_measurements(&base, &token, metric, 10).await {
                    Ok(rs) => rows.extend(rs),
                    Err(_) => {}
                }
            }
            rows.sort_by(|a, b| {
                b.ts_ms()
                    .unwrap_or(0.0)
                    .partial_cmp(&a.ts_ms().unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            set_measurements.set(rows);
        });
    });

    // Persisted action log (Skopos): fetched on tab load + after every apply.
    let refresh_actions = move || {
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            match fetch_actions(&base, &token).await {
                Ok(a) => set_actions.set(a),
                Err(e) => log_event("api", &format!("action log fetch failed: {e}")),
            }
        });
    };
    create_effect(move |_| {
        if current_tab.get() == Tab::Skopos {
            refresh_actions();
        }
    });

    let revert_action_fn = move |id: String| {
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            match revert_action(&base, &token, &id).await {
                Ok(_) => {
                    log_event("action", &format!("reverted action {id}"));
                    refresh_actions();
                    refresh();
                    meas_refresh.set(meas_refresh.get() + 1);
                    nut_refresh.set(nut_refresh.get() + 1);
                }
                Err(e) => set_error.set(Some(e)),
            }
        });
    };

    // LOG button for the MEASUREMENTS section: POST then refresh the list.
    let log_measurement = move |metric: &'static str| {
        let val: f64 = if metric == "weight_kg" {
            meas_weight.get().trim().parse().unwrap_or(0.0)
        } else {
            meas_body_fat.get().trim().parse().unwrap_or(0.0)
        };
        if val <= 0.0 {
            set_error.set(Some("enter a value first".to_string()));
            return;
        }
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            match post_measurement(&base, &token, metric, val, None).await {
                Ok(_) => {
                    log_event("measurement", &format!("logged {metric} {val:.1}"));
                    if metric == "weight_kg" {
                        set_meas_weight.set(String::new());
                    } else {
                        set_meas_body_fat.set(String::new());
                    }
                    refresh_actions();
                    meas_refresh.set(meas_refresh.get() + 1);
                }
                Err(e) => set_error.set(Some(e)),
            }
        });
    };

    // -- Nomoi import: parse picked file client-side, preview, POST /import --

    let on_import_file = move |ev: web_sys::Event| {
        let file = event_target_files(&ev).and_then(|mut v| v.pop());
        let Some(file) = file else { return };
        let name = file.name();
        set_import_name.set(name.clone());
        set_import_result.set(None);
        set_importing.set(true);
        spawn_local(async move {
            let text = match read_file_text(&file).await {
                Ok(t) => t,
                Err(e) => {
                    set_import_buf.set(Vec::new());
                    set_import_preview.set(Vec::new());
                    set_import_errors.set(vec![e]);
                    set_importing.set(false);
                    return;
                }
            };
            let lower = name.to_ascii_lowercase();
            let (samples, errors) = if lower.ends_with(".gpx") {
                parse_import_gpx(&text)
            } else if lower.ends_with(".json") {
                parse_import_json(&text)
            } else {
                parse_import_csv(&text)
            };
            set_import_buf.set(samples.clone());
            set_import_preview.set(
                samples
                    .iter()
                    .take(5)
                    .map(|s| {
                        (
                            s.get("timestamp").and_then(|v| v.as_str()).unwrap_or("").chars().take(16).collect::<String>(),
                            s.get("metric").and_then(|v| v.as_str()).unwrap_or("?").to_string(),
                            s.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0),
                        )
                    })
                    .collect(),
            );
            set_import_errors.set(errors);
            set_importing.set(false);
        });
    };

    let do_import = move |_| {
        let samples = import_buf.get();
        if samples.is_empty() {
            set_import_result.set(Some((0, 0, vec!["nothing to import — pick a file first".to_string()])));
            return;
        }
        let (base, token, source, device) = (
            base.get(),
            token.get(),
            import_source.get(),
            import_device.get().trim().to_string(),
        );
        set_importing.set(true);
        spawn_local(async move {
            let device_id = if device.is_empty() { None } else { Some(device) };
            match import_samples(&base, &token, &source, device_id.as_deref(), &samples).await {
                Ok(r) => {
                    set_import_result.set(Some((r.inserted, r.skipped, r.errors)));
                    log_event("import", &format!("POST /import ({source}) · {} inserted · {} skipped", r.inserted, r.skipped));
                    refresh();
                }
                Err(e) => set_import_result.set(Some((0, 0, vec![e]))),
            }
            set_importing.set(false);
        });
    };

    // -- Pythia oracle (AI chat panel) ----------------------------------------

    // Compact state digest for the panel strip (recomputed when the panel
    // opens).
    let oracle_digest_lines = move || -> Vec<(String, String)> {
        let ready = readiness.get().and_then(|d| d.last().cloned());
        let be = body_energy.get();
        let sleep_h = sleep.get().last().map(|s| s.sleep_seconds / 3600.0);
        let types_ = types.get();
        let type_name = |id: &Option<String>| {
            match id.as_deref() {
                Some(tid) => types_
                    .iter()
                    .find(|t| t.id == *tid)
                    .map(|t| t.name.clone())
                    .unwrap_or_else(|| "Workout".to_string()),
                None => "Workout".to_string(),
            }
        };
        let ss = sessions.get();
        let session_line = if let Some(a) = ss.iter().find(|s| s.status == "active") {
            let start = ms_from_iso(&a.start_time).map(fmt_time).unwrap_or_default();
            format!("{} · LIVE since {start}", type_name(&a.type_id))
        } else if let Some(w) = ss
            .iter()
            .filter(|s| s.status != "active")
            .max_by(|a, b| {
                ms_from_iso(&a.start_time)
                    .unwrap_or(0.0)
                    .partial_cmp(&ms_from_iso(&b.start_time).unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        {
            let start = ms_from_iso(&w.start_time).map(fmt_time).unwrap_or_default();
            let dur = match (ms_from_iso(&w.start_time), w.end_time.as_deref().and_then(ms_from_iso)) {
                (Some(s), Some(e)) if e > s => format!(" · {:.0}m", (e - s) / 60_000.0),
                _ => String::new(),
            };
            format!("{} · {start}{dur}", type_name(&w.type_id))
        } else {
            "—".to_string()
        };
        vec![
            ("READINESS".into(), ready.map(|r| format!("{:.0} / 300", r.score)).unwrap_or_else(|| "—".into())),
            ("DYNAMIS".into(), be.as_ref().map(|b| format!("{:.0} / 300", b.score)).unwrap_or_else(|| "—".into())),
            ("PONOS".into(), be.as_ref().map(|b| format!("{:.0} pts", b.stress)).unwrap_or_else(|| "—".into())),
            ("SLEEP".into(), sleep_h.map(|h| format!("{h:.1} h")).unwrap_or_else(|| "—".into())),
            ("GOALS".into(), format!("{:.0}h · {:.0}kg · {:.0}%", target_intensity.get(), target_weight.get(), target_body_fat.get())),
            ("MEASURE".into(), {
                let w = measurements.get().into_iter().find(|m| m.metric == "weight_kg").map(|m| format!("{:.1} kg", m.value)).unwrap_or_else(|| "—".to_string());
                let f = measurements.get().into_iter().find(|m| m.metric == "body_fat_pct").map(|m| format!("{:.1} %", m.value)).unwrap_or_else(|| "—".to_string());
                format!("{w} · {f}")
            }),
            ("SESSION".into(), session_line),
            ("NUTRITION".into(), nut_daily.get().map(|n| format!("{:.0} kcal · {:.0} ml", n.kcal, n.water_ml)).unwrap_or_else(|| "—".into())),
        ]
    };

    // Bounded (~2KB) state context sent with every chat turn.
    let oracle_context = move || -> Value {
        let ready = readiness.get().and_then(|d| d.last().cloned());
        let be = body_energy.get();
        let sleep_h = sleep.get().last().map(|s| s.sleep_seconds / 3600.0);
        let types_ = types.get();
        let type_name = |id: &Option<String>| {
            match id.as_deref() {
                Some(tid) => types_
                    .iter()
                    .find(|t| t.id == *tid)
                    .map(|t| t.name.clone())
                    .unwrap_or_else(|| "unknown".to_string()),
                None => "unknown".to_string(),
            }
        };
        let ss = sessions.get();
        let active = ss.iter().find(|s| s.status == "active").map(|s| {
            json!({
                "type": type_name(&s.type_id),
                "startMs": ms_from_iso(&s.start_time).unwrap_or(0.0),
                "endMs": s.end_time.as_deref().and_then(ms_from_iso),
            })
        });
        let last_done = ss
            .iter()
            .filter(|s| s.status != "active")
            .max_by(|a, b| {
                ms_from_iso(&a.start_time)
                    .unwrap_or(0.0)
                    .partial_cmp(&ms_from_iso(&b.start_time).unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        let last_workout = last_done.map(|w| {
            let start = ms_from_iso(&w.start_time).unwrap_or(0.0);
            let dur = w
                .end_time
                .as_deref()
                .and_then(ms_from_iso)
                .and_then(|e| (e > start).then(|| (e - start) / 60_000.0));
            json!({ "type": type_name(&w.type_id), "durationMin": dur })
        });
        let sc = series.get();
        json!({
            "date": today_str(),
            "rangeDays": days.get(),
            "readinessScore": ready.map(|r| r.score),
            "battery": be.as_ref().map(|b| b.score),
            "stress": be.as_ref().map(|b| b.stress),
            "sleepH": sleep_h,
            "activeSession": active,
            "lastWorkout": last_workout,
            "nutritionToday": nut_daily.get().map(|n| json!({
                "kcal": n.kcal,
                "protein": n.protein,
                "carbs": n.carbs,
                "fat": n.fat,
                "waterMl": n.water_ml,
            })),
            "targets": {
                "steps": target_steps.get(),
                "kcal": target_kcal.get(),
                "sleepH": target_sleep.get(),
                "intensityHoursPerWeek": target_intensity.get(),
                "weightKg": target_weight.get(),
                "bodyFatPct": target_body_fat.get(),
            },
            "weekIntensityH": week_intensity_h(),
            "measurements": {
                "weightKg": measurements.get().iter().find(|m| m.metric == "weight_kg").map(|m| m.value),
                "bodyFatPct": measurements.get().iter().find(|m| m.metric == "body_fat_pct").map(|m| m.value),
            },
            "series": { "heartRate": sc.heart_rate, "steps": sc.steps, "calories": sc.calories },
            "aiProviderModel": ai_model.get(),
        })
    };

    let oracle_toggle = move |_| {
        let open = !oracle_open.get();
        if open {
            set_oracle_digest.set(oracle_digest_lines());
            set_oracle_error.set(None);
        }
        set_oracle_open.set(open);
    };

    // Send one chat turn (also called from the input's Enter key).
    let oracle_send = move || {
        let text = oracle_input.get().trim().to_string();
        if text.is_empty() || oracle_busy.get() || ai_model.get().trim().is_empty() {
            return;
        }
        let base = base.get();
        let token = token.get();
        let context = oracle_context();
        let id = oracle_msg_seq.get();
        oracle_msg_seq.set(id + 1);
        set_oracle_msgs.update(|v| v.push(OracleMsg { id, role: "user", content: text }));
        set_oracle_input.set(String::new());
        set_oracle_error.set(None);
        // History = the last 12 messages (including the one just pushed).
        let history: Vec<ChatMessage> = oracle_msgs
            .get()
            .into_iter()
            .rev()
            .take(12)
            .rev()
            .map(|m| ChatMessage { role: m.role.to_string(), content: m.content })
            .collect();
        set_oracle_busy.set(true);
        spawn_local(async move {
            match ai_chat(&base, &token, &history, &context).await {
                Ok(resp) => {
                    let id = oracle_msg_seq.get();
                    oracle_msg_seq.set(id + 1);
                    set_oracle_msgs.update(|v| v.push(OracleMsg { id, role: "assistant", content: resp.reply }));
                    set_oracle_proposals.set(
                        resp
                            .proposals
                            .into_iter()
                            .map(|p| {
                                let input = proposal_input_for(&p.key, &p.proposed);
                                OracleProposalRow {
                                    key: p.key,
                                    label: p.label,
                                    current: fmt_value(&p.current),
                                    reason: p.reason,
                                    draft: proposal_draft(&p.proposed),
                                    input,
                                    checked: true,
                                    action: p.action.clone(),
                                    metric: p.metric.clone(),
                                    value: p.value,
                                }
                            })
                            .collect(),
                    );
                }
                Err(e) => {
                    let id = oracle_msg_seq.get();
                    oracle_msg_seq.set(id + 1);
                    set_oracle_msgs.update(|v| v.push(OracleMsg { id, role: "assistant", content: format!("[PYTHIA ERROR] {e}") }));
                    set_oracle_proposals.set(Vec::new());
                }
            }
            set_oracle_busy.set(false);
        });
    };

    // Keep the thread pinned to the newest message.
    create_effect(move |_| {
        let _ = oracle_msgs.get().len();
        let _ = oracle_busy.get();
        if let Some(el) = oracle_thread_ref.get() {
            el.set_scroll_top(el.scroll_height());
        }
    });

    // Accept the checked proposals: read the settings blob, apply each
    // (possibly adjusted) value at its dotted key path, PUT it back, then
    // refresh so KPIs/charts reflect the change.
    let oracle_accept = move |_| {
        let accepted: Vec<OracleProposalRow> = oracle_proposals
            .get()
            .into_iter()
            .filter(|p| p.checked)
            .collect();
        set_oracle_proposals.set(Vec::new());
        if accepted.is_empty() {
            return;
        }
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            let mut settings = fetch_settings(&base, &token).await.unwrap_or(Value::Null);
            if !settings.is_object() {
                settings = json!({});
            }
            let mut applied = 0usize;
            let mut put_needed = false;
            for row in &accepted {
                match row.action.as_deref() {
                    // Action proposals: POST data instead of a settings PUT.
                    Some("measurement") => {
                        let metric = row.metric.clone().unwrap_or_else(|| "weight_kg".to_string());
                        let value = row.draft.trim().parse::<f64>().unwrap_or(row.value.unwrap_or(0.0));
                        match post_measurement(&base, &token, &metric, value, None).await {
                            Ok(_) => applied += 1,
                            Err(e) => set_error.set(Some(e)),
                        }
                    }
                    Some("meal") => {
                        let amount = row.draft.trim().parse::<f64>().unwrap_or(row.value.unwrap_or(0.0));
                        let body = json!({
                            "kind": "food",
                            "amount": amount,
                            "consumedAt": iso_from_ms(js_sys::Date::now()),
                            "note": row.reason,
                        });
                        match add_nutrition(&base, &token, &body).await {
                            Ok(_) => applied += 1,
                            Err(e) => set_error.set(Some(e)),
                        }
                    }
                    _ => {
                        let path = row.key.strip_prefix("settings.").unwrap_or(&row.key);
                        let value: Value = match row.input {
                            ProposalInput::Bool => json!(row.draft.trim() == "true"),
                            ProposalInput::Days => row.draft.trim().parse::<i64>().map(Value::from).unwrap_or(Value::Null),
                            ProposalInput::Number => row.draft.trim().parse::<f64>().map(Value::from).unwrap_or(Value::Null),
                            ProposalInput::Text => json!(row.draft),
                        };
                        if set_path(&mut settings, path, value) {
                            applied += 1;
                            put_needed = true;
                        }
                    }
                }
            }
            if put_needed {
                match put_settings(&base, &token, &settings).await {
                    Ok(_) => {}
                    Err(e) => set_error.set(Some(e)),
                }
            }
            log_event("ai", &format!("oracle accepted {applied} changes"));
            set_oracle_open.set(false);
            // Mirror accepted values into the local signals that drive the
            // UI (settings only load once per session). Action proposals have
            // no settings key to mirror.
            for row in &accepted {
                if row.action.is_some() {
                    continue;
                }
                let draft = &row.draft;
                match (row.key.strip_prefix("settings."), row.input) {
                    (Some("rangeDays"), ProposalInput::Days) => {
                        if let Ok(d) = draft.trim().parse::<i64>() {
                            set_days.set(d);
                        }
                    }
                    (Some("series.heartRate"), ProposalInput::Bool) => set_series.update(|s| s.heart_rate = draft.trim() == "true"),
                    (Some("series.steps"), ProposalInput::Bool) => set_series.update(|s| s.steps = draft.trim() == "true"),
                    (Some("series.calories"), ProposalInput::Bool) => set_series.update(|s| s.calories = draft.trim() == "true"),
                    (Some("targets.steps"), ProposalInput::Number) => {
                        if let Ok(v) = draft.trim().parse::<i64>() {
                            set_target_steps.set(v);
                        }
                    }
                    (Some("targets.kcal"), ProposalInput::Number) => {
                        if let Ok(v) = draft.trim().parse::<i64>() {
                            set_target_kcal.set(v);
                        }
                    }
                    (Some("targets.sleepH"), ProposalInput::Number) => {
                        if let Ok(v) = draft.trim().parse::<f64>() {
                            set_target_sleep.set(v);
                        }
                    }
                    (Some("targets.intensityHoursPerWeek"), ProposalInput::Number) => {
                        if let Ok(v) = draft.trim().parse::<f64>() {
                            set_target_intensity.set(v);
                        }
                    }
                    (Some("targets.weightKg"), ProposalInput::Number) => {
                        if let Ok(v) = draft.trim().parse::<f64>() {
                            set_target_weight.set(v);
                        }
                    }
                    (Some("targets.bodyFatPct"), ProposalInput::Number) => {
                        if let Ok(v) = draft.trim().parse::<f64>() {
                            set_target_body_fat.set(v);
                        }
                    }
                    (Some("aiProvider.model"), ProposalInput::Text) => set_ai_model.set(draft.clone()),
                    _ => {}
                }
            }
            refresh();
            refresh_actions();
            meas_refresh.set(meas_refresh.get() + 1);
            nut_refresh.set(nut_refresh.get() + 1);
        });
    };

    let oracle_dismiss = move |_| set_oracle_proposals.set(Vec::new());

    // Contextual PYTHIA: open the oracle with a section-scoped starter prompt
    // and refresh the digest so the model sees the relevant current values.
    const ORACLE_FLOURISH: &str = "Use the app state. Give concrete values. Never ask clarifying questions — if data is missing, use a sensible default and say so. Metric units: kg, kcal, hours, %.";
    let oracle_ask = move |prompt: String| {
        set_oracle_input.set(prompt);
        set_oracle_digest.set(oracle_digest_lines());
        set_oracle_error.set(None);
        set_oracle_open.set(true);
    };
    let oracle_ask_nutrition = move |_| oracle_ask(format!("Add a meal with estimated kcal + macros — e.g. describe what I ate. {ORACLE_FLOURISH}"));
    let oracle_ask_goals = move |_| oracle_ask(format!("Set my goals: weekly intensity (hours), body weight (kg), and body fat (%). {ORACLE_FLOURISH}"));
    let oracle_ask_measure = move |_| oracle_ask(format!("Log my weight and body fat via AI. {ORACLE_FLOURISH}"));
    let oracle_ask_weight = move |_| {
        let v = meas_weight.get().trim().to_string();
        let prompt = if v.is_empty() {
            format!("Log my weight in kg — the value is in the input. {ORACLE_FLOURISH}")
        } else {
            format!("Log my weight as {v} kg. {ORACLE_FLOURISH}")
        };
        oracle_ask(prompt);
    };
    let oracle_ask_body_fat = move |_| {
        let v = meas_body_fat.get().trim().to_string();
        let prompt = if v.is_empty() {
            format!("Log my body fat in % — the value is in the input. {ORACLE_FLOURISH}")
        } else {
            format!("Log my body fat as {v} %. {ORACLE_FLOURISH}")
        };
        oracle_ask(prompt);
    };
    let oracle_ask_ai_config = move |_| oracle_ask(format!("Help me configure the AI provider (base URL, model, API key) so PYTHIA can answer. {ORACLE_FLOURISH}"));

    // -- Nomoi: AI provider test + save ----------------------------------------

    // Placeholder base URL per provider.
    let ai_placeholder = move || -> &'static str {
        match ai_provider.get().as_str() {
            "ollama" => "http://localhost:11434",
            "openai" => "https://api.openai.com/v1",
            _ => "http://localhost:8080/v1",
        }
    };

    let test_ai_provider = move |_| {
        let provider = ai_provider.get();
        let b_url = ai_base.get();
        let model = ai_model.get();
        let key = ai_key.get();
        let base = base.get();
        let token = token.get();
        set_ai_testing.set(true);
        set_ai_test_result.set(None);
        spawn_local(async move {
            let out = ai_test(&base, &token, &provider, &b_url, &model, &key).await;
            set_ai_testing.set(false);
            set_ai_test_result.set(Some(match out {
                Ok(v) if v.get("ok").and_then(|o| o.as_bool()) == Some(true) => {
                    let reply = v.get("reply").and_then(|r| r.as_str()).unwrap_or("").to_string();
                    (true, if reply.is_empty() { "PROVIDER REACHABLE".to_string() } else { reply })
                }
                Ok(v) => (
                    false,
                    v.get("error")
                        .and_then(|e| e.as_str())
                        .unwrap_or("provider rejected the test")
                        .to_string(),
                ),
                Err(e) => (false, e),
            }));
        });
    };

    // Read-merge-PUT: only settings.aiProvider changes, other keys preserved.
    let save_ai_provider = move |_| {
        let provider = ai_provider.get();
        let b_url = ai_base.get();
        let model = ai_model.get();
        let key = ai_key.get();
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            let mut ai = json!({ "provider": provider, "baseUrl": b_url, "model": model, "systemPrompt": ai_sys_prompt.get() });
            if !key.trim().is_empty() {
                ai["apiKey"] = json!(key.trim());
            }
            let mut settings = fetch_settings(&base, &token).await.unwrap_or(Value::Null);
            if !settings.is_object() {
                settings = json!({});
            }
            settings["aiProvider"] = ai;
            match put_settings(&base, &token, &settings).await {
                Ok(_) => {
                    log_event("settings", "saved aiProvider");
                    set_ai_test_result.set(None);
                }
                Err(e) => set_error.set(Some(e)),
            }
        });
    };

    let reset_ai_sys_prompt = move |_| set_ai_sys_prompt.set(DEFAULT_SYSTEM_PROMPT.to_string());

    // -----------------------------------------------------------------------
    view! {
        <div class="app">
            <header class="header">
                <div class="brand">
                    <div class="brand-row">
                        <h1 class:flip=move || brand_flip.get() on:click=move |_| { set_brand_flip.update(|v| *v = !*v); }>
                            <span class="wm-state wm-latin">"EPHORIX"</span>
                            <span class="wm-state wm-greek">"ΕΦΟΡΙΞ"</span>
                        </h1>
                    </div>
                    <span class="sub">"ΑΓΩΓΗ · TRAINING COMMAND"</span>
                </div>
                <svg class="brand-lambda" viewBox="0 0 120 100" role="img" aria-label="Spartan lambda">
                    <path fill="#e53935" d="M60 12 L34 73 L26 73 L26 80 L48 80 Z M60 12 L86 73 L94 73 L94 80 L72 80 Z M60 12 L62.5 20 L62.5 85 L57.5 85 L57.5 20 Z M60 85 L66 90.5 L60 96 L54 90.5 Z" />
                    <path fill="none" stroke="#7b0000" stroke-width="2" d="M50.5 40 L37 71 M53.5 40 L40 71 M69.5 40 L82.7 71 M66.5 40 L79.7 71" />
                    <path fill="none" stroke="#ff5252" stroke-width="2" d="M60 18 L60 84" />
                    <path fill="#ff5252" d="M60 5 L64 9 L60 13 L56 9 Z" />
                </svg>
            </header>

            <nav class="tabs">
                {Tab::ALL.iter().map(|t| {
                    let t = *t;
                    view! {
                        <button class="tab" class:on=move || current_tab.get() == t on:click=move |_| { set_current_tab.set(t); log_event("ui", &format!("tab → {}", t.label())); }>
                            <span class="gr">{t.label()}</span>
                            <span class="en">{t.english()}</span>
                        </button>
                    }
                }).collect_view()}
            </nav>

            <div class="meander-rule"></div>

            <Show when=move || error.get().is_some() fallback=|| ()>
                <div class="banner-error">{move || error.get().unwrap_or_default()}</div>
            </Show>

            <main>
                {move || match current_tab.get() {
                    Tab::Gymnasia => view! {
                        <TabHero tab=Tab::Gymnasia />
                        <div class="kpi" style="grid-template-columns: repeat(6, 1fr)">
                            <div class="kpi-chip">
                                <span class="kpi-label">"HEART RATE"</span>
                                <div class="kpi-value">
                                    {move || points.get().iter().rev().find_map(|p| p.heart_rate)
                                        .map(|v| format!("{v:.0}")).unwrap_or_else(|| "—".to_string())}
                                    <span class="unit">"BPM"</span>
                                </div>
                            </div>
                            <div class="kpi-chip">
                                <span class="kpi-label">"STEPS"</span>
                                <div class="kpi-value">
                                    {move || format!("{:.0}", points.get().iter().filter_map(|p| p.steps).sum::<i64>())}
                                    <span class="unit">"RANGE"</span>
                                </div>
                            </div>
                            <div class="kpi-chip">
                                <span class="kpi-label">"ACTIVE KCAL"</span>
                                <div class="kpi-value">
                                    {move || format!("{:.0}", points.get().iter().filter_map(|p| p.active_calories).sum::<f64>())}
                                    <span class="unit">"RANGE"</span>
                                </div>
                            </div>
                            <div class="kpi-chip">
                                <span class="kpi-label">"SLEEP"</span>
                                <div class="kpi-value">
                                    {move || {
                                        let h = sleep.get().iter().map(|s| s.sleep_seconds).sum::<f64>() / 3600.0;
                                        format!("{h:.1}")
                                    }}
                                    <span class="unit">"H"</span>
                                </div>
                            </div>
                            <div class="kpi-chip">
                                <span class="kpi-label">"DYNAMIS"</span>
                                <div class="kpi-value">
                                    {move || body_energy.get().map(|b| format!("{:.0}", b.score)).unwrap_or_else(|| "—".to_string())}
                                    <span class="unit">"/300"</span>
                                </div>
                            </div>
                            <div class="kpi-chip">
                                <span class="kpi-label">"READINESS"</span>
                                <div class="kpi-value">
                                    {move || readiness.get().and_then(|d| d.last().map(|t| format!("{:.0}", t.score)))
                                        .unwrap_or_else(|| "—".to_string())}
                                    <span class="unit">"/300"</span>
                                </div>
                            </div>
                        </div>
                        <section class="panel">
                            <div class="panel-head">
                                <h2>"RAW METRICS / AGOGE OVERLAY"</h2>
                                <div class="range-buttons">
                                    {RANGES.iter().map(|(d, label)| {
                                        let d = *d;
                                        view! {
                                            <button class="pill" class:on=move || active_range.get() == Some(d) on:click=move |_| {
                                                set_range_days(d);
                                                active_range.set(Some(d));
                                                // The preset also resets the zoom (refetch follows via
                                                // the days signal) and clears any selection.
                                                reset_zoom.set(reset_zoom.get() + 1);
                                                clear_sel.set(clear_sel.get() + 1);
                                                set_selection.set(None);
                                            }>{*label}</button>
                                        }
                                    }).collect_view()}
                                    <span class="muted cursor-readout">
                                        "CURSOR "
                                        {move || cursor.get().map(fmt_time).unwrap_or_else(|| "—".to_string())}
                                    </span>
                                </div>
                            </div>
                            <div class="chart-toolbar">
                                <div class="mode-toggle">
                                    <button class:on=move || zoom_mode.get() on:click=move |_| {
                                        // Switching back to ZOOM clears any pending selection.
                                        if !zoom_mode.get() {
                                            clear_sel.set(clear_sel.get() + 1);
                                            set_selection.set(None);
                                        }
                                        set_zoom_mode.set(true);
                                    }>"ZOOM"</button>
                                    <button class:on=move || !zoom_mode.get() on:click=move |_| set_zoom_mode.set(false)>"SELECT"</button>
                                </div>
                                <Show when=move || !zoom_mode.get() fallback=|| ()>
                                    <Show when=move || selection.get().is_some() || point_ts.get().is_some() fallback=|| ()>
                                        <div class="select-box">
                                            <Show when=move || selection.get().is_some() fallback=|| ()>
                                                <div class="select-actions">
                                                    <button class="btn" on:click=create_from_selection>"AGOGE FROM SELECTION"</button>
                                                    <span class="muted selection-readout">
                                                        "SELECTION: "
                                                        {move || selection.get().map(|(f, t)| format!("{} → {}", fmt_time(f.min(t)), fmt_time(f.max(t)))).unwrap_or_else(|| "—".to_string())}
                                                    </span>
                                                </div>
                                            </Show>
                                            <Show when=move || point_ts.get().is_some() fallback=|| ()>
                                                <div class="select-actions">
                                                    <button class="btn" on:click=agoge_start>"AGOGE START"</button>
                                                    <button class="btn" on:click=agoge_end>"AGOGE END"</button>
                                                    <span class="muted point-readout">
                                                        "POINT: "
                                                        {move || point_ts.get().map(fmt_time).unwrap_or_else(|| "—".to_string())}
                                                    </span>
                                                </div>
                                            </Show>
                                        </div>
                                    </Show>
                                </Show>
                            </div>
                            <TimelineChart
                                points=points
                                sessions=sessions
                                types=types
                                nutrition=nutrition
                                sleep=sleep
                                battery=battery_series
                                series=series
                                selection=selection
                                set_selection=set_selection
                                point_ts=point_ts
                                cursor=set_cursor
                                pick_mode=pick_mode
                                zoom_mode=zoom_mode
                                clear_trigger=clear_sel
                                reset_zoom=reset_zoom
                                on_click_at=Callback::new(move |ts| handle_chart_click(ts))
                                on_zoom=Callback::new(move |_| active_range.set(None))
                                on_ready=Callback::new(move |id| handle_chart_ready(id))
                                on_session_click=Callback::new(move |sid| handle_session_click(sid))
                            />
                            <div class="agoge-type-bar">
                                <span class="agoge-type-label">"AGOGE TYPE"</span>
                                <select on:change=move |ev| set_selected_type.set(option_value(&ev))>
                                    <option value="">"UNDEFINED"</option>
                                    <For each=move || types.get() key=|t| t.id.clone() let:t>
                                        <option value=t.id.clone()>{t.name.clone()}</option>
                                    </For>
                                </select>
                            </div>
                            <div class="session-strip">
                                <For each=move || {
                                    let _ = types.get();
                                    let mut ss = sessions.get();
                                    ss.sort_by(|a, b| {
                                        ms_from_iso(&b.start_time)
                                            .unwrap_or(0.0)
                                            .partial_cmp(&ms_from_iso(&a.start_time).unwrap_or(0.0))
                                            .unwrap_or(std::cmp::Ordering::Equal)
                                    });
                                    ss
                                } key=|s| s.id.clone() let:s>
                                    <SessionChip session=s types=types on_open=Callback::new(move |s: AgogeSession| handle_session_click(s.id.clone())) />
                                </For>
                            </div>
                            <Show when=move || selected_session.get().is_some() fallback=|| ()>
                                {move || {
                                    let s = selected_session.get().unwrap();
                                    view! {
                                        <div key=s.id.clone()>
                                            <SessionDetails
                                                session=s
                                                base=base.get_untracked()
                                                token=token.get_untracked()
                                                types=types
                                                pick_field=pick_field
                                                on_type=Callback::new(move |t: (String, String)| save_session_type(t.0, t.1))
                                                on_pick=Callback::new(move |f| toggle_pick(f))
                                                on_duration=Callback::new(move |d: (String, String)| apply_duration(d.0, d.1))
                                                on_delete=Callback::new(move |id| delete_selected_session(id))
                                                on_close=Callback::new(move |id| close_selected_session(id))
                                            />
                                        </div>
                                    }
                                }}
                            </Show>
                        </section>
                    }.into_view(),

                    Tab::Agoges => view! {
                        <TabHero tab=Tab::Agoges />
                        <div class="agoges-layout">
                            <aside class="agoges-sidebar">
                                <div class="sidebar-head">
                                    <span class="sidebar-title">"AGOGES"</span>
                                    <button class="btn small" on:click=move |_| set_selected_id.set(None)>"+"</button>
                                </div>
                                <ul class="sidebar-list">
                                    <For each=move || types.get() key=|t| t.id.clone() let:t>
                                        {move || {
                                            let tid = t.id.clone();
                                            let tid_click = tid.clone();
                                            let tname = t.name.clone();
                                            let tcolor = t.color_code.clone();
                                            let tcat = t.category.clone();
                                            view! {
                                                <li class="sidebar-item" class:on=move || selected_id.get().as_deref() == Some(tid.as_str()) on:click=move |_| set_selected_id.set(Some(tid_click.clone()))>
                                                    <span class="dot" style=format!("background:{tcolor}")></span>
                                                    <span class="si-name">{tname}</span>
                                                    <span class="si-cat">{tcat.to_uppercase()}</span>
                                                </li>
                                            }
                                        }}
                                    </For>
                                </ul>
                            </aside>
                            <section class="agoges-config panel">
                                {move || {
                                    let sel = selected_id.get();
                                    let ty = sel.as_ref().and_then(|id| types.get().into_iter().find(|t| &t.id == id));
                                    view! {
                                        <div key=sel.clone()>
                                            <AgogeConfigForm
                                                ty=ty
                                                on_save=Callback::new(move |(n, c, i, cat, cfg): (String, String, String, String, Value)| {
                                                    if let Some(id) = sel.clone() {
                                                        update_type(id, n, c, i, cat, cfg);
                                                    } else {
                                                        create_type(n, c, i, cat, cfg);
                                                    }
                                                    set_selected_id.set(None);
                                                })
                                                on_delete=Callback::new(move |id: String| {
                                                    delete_type(id);
                                                    set_selected_id.set(None);
                                                })
                                            />
                                        </div>
                                    }
                                }}
                            </section>
                        </div>
                    }.into_view(),

                    Tab::Askesis => view! {
                        <TabHero tab=Tab::Askesis />
                        <section class="panel">
                            <div class="panel-head"><h2>"METRICS SUMMARY"</h2><span class="muted">"over the selected range"</span></div>
                            <ul class="list metrics">
                                <li class="row"><span class="row-name">"Average heart rate"</span><span class="metric-val">{move || { let p = points.get(); let hr: Vec<f64> = p.iter().filter_map(|x| x.heart_rate).collect(); if hr.is_empty() { "—".to_string() } else { format!("{:.0} bpm", hr.iter().sum::<f64>() / hr.len() as f64) } }}</span></li>
                                <li class="row"><span class="row-name">"Peak heart rate"</span><span class="metric-val">{move || { let peak = points.get().iter().filter_map(|x| x.heart_rate).fold(0.0, f64::max); if peak > 0.0 { format!("{peak:.0} bpm") } else { "—".to_string() } }}</span></li>
                                <li class="row"><span class="row-name">"Total steps"</span><span class="metric-val">{move || format!("{:.0}", points.get().iter().filter_map(|x| x.steps).sum::<i64>())}</span></li>
                                <li class="row"><span class="row-name">"Active calories"</span><span class="metric-val">{move || format!("{:.0} kcal", points.get().iter().filter_map(|x| x.active_calories).sum::<f64>())}</span></li>
                                <li class="row"><span class="row-name">"Sleep"</span><span class="metric-val">{move || format!("{:.1} h", sleep.get().iter().map(|s| s.sleep_seconds).sum::<f64>() / 3600.0)}</span></li>
                                <li class="row"><span class="row-name">"Workouts (recorded)"</span><span class="metric-val">{move || format!("{}", sessions.get().len())}</span></li>
                                <li class="row"><span class="row-name">"Detected (proposed)"</span><span class="metric-val">{move || format!("{}", detections.get().iter().filter(|d| d.status == "proposed").count())}</span></li>
                            </ul>
                        </section>
                    }.into_view(),

                    Tab::Syntaxis => view! {
                        <TabHero tab=Tab::Syntaxis />
                        <section class="panel">
                            <div class="panel-head">
                                <h2>"DETECTED ACTIVITIES"</h2>
                                <span class="muted">{move || format!("{} proposed · accept or reject", detections.get().iter().filter(|d| d.status == "proposed").count())}</span>
                            </div>
                            <Show when=move || detections.get().iter().any(|d| d.status == "proposed") fallback=|| view! { <p class="muted">"No proposed activities in this range. Train with the watch or ingest data and they will surface here."</p> }>
                                <ul class="list detections">
                                    <For each=move || detections.get().into_iter().filter(|d| d.status == "proposed") key=|d| d.id.clone() let:d>
                                        {move || {
                                            let label = format!("{} · {} – {} · peak {} bpm", d.proposed_type_name.clone().unwrap_or_else(|| "Activity".to_string()), fmt_time(d.start), fmt_time(d.end), d.peak_hr as i64);
                                            let aid = d.id.clone();
                                            let rid = d.id.clone();
                                            view! {
                                                <li class="row detection-row">
                                                    <span class="row-name">{label}</span>
                                                    <button class="btn small" on:click=move |_| accept_det(aid.clone())>"ACCEPT"</button>
                                                    <button class="btn small" on:click=move |_| reject_det(rid.clone())>"REJECT"</button>
                                                </li>
                                            }
                                        }}
                                    </For>
                                </ul>
                            </Show>
                        </section>
                        <section class="panel">
                            <div class="panel-head"><h2>"AGOGE SESSIONS"</h2><span class="muted">{move || format!("{} total", sessions.get().len())}</span></div>
                            <ul class="list">
                                <For each=move || { let _ = types.get(); sessions.get() } key=|s| s.id.clone() let:s>
                                    <SessionRow session=s types=types on_delete=Callback::new(move |id: String| delete_session(id)) />
                                </For>
                            </ul>
                        </section>
                    }.into_view(),

                    Tab::Leonidas => view! {
                        <TabHero tab=Tab::Leonidas />
                        <section class="panel">
                            <div class="panel-head"><h2>"LEONIDAS — TARGETS"</h2><span class="muted">"be like Leonidas"</span></div>

                            <div class="goal-section">
                                <h3 class="section-h3">"TRAINING TARGETS"</h3>
                                <div class="target-row">
                                    <span class="row-name">"Steps / day"</span>
                                    <input prop:value=move || target_steps.get().to_string() on:input=move |ev| set_target_steps.set(event_target_value(&ev).parse().unwrap_or(0)) />
                                    <div class="bar">
                                        <div class="bar-fill" style=move || format!("width: {}%", (points.get().iter().filter_map(|p| p.steps).sum::<i64>() as f64 / target_steps.get().max(1) as f64 * 100.0).min(100.0))></div>
                                    </div>
                                </div>
                                <div class="target-row">
                                    <span class="row-name">"Active kcal / day"</span>
                                    <input prop:value=move || target_kcal.get().to_string() on:input=move |ev| set_target_kcal.set(event_target_value(&ev).parse().unwrap_or(0)) />
                                    <div class="bar"><div class="bar-fill" style=move || format!("width: {}%", (points.get().iter().filter_map(|p| p.active_calories).sum::<f64>() / target_kcal.get().max(1) as f64 * 100.0).min(100.0))></div></div>
                                </div>
                                <div class="target-row">
                                    <span class="row-name">"Sleep / night (h)"</span>
                                    <input prop:value=move || target_sleep.get().to_string() on:input=move |ev| set_target_sleep.set(event_target_value(&ev).parse().unwrap_or(0.0)) />
                                    <div class="bar"><div class="bar-fill" style=move || format!("width: {}%", (sleep.get().iter().map(|s| s.sleep_seconds).sum::<f64>() / 3600.0 / target_sleep.get().max(0.1) * 100.0).min(100.0))></div></div>
                                </div>
                                <div class="target-row">
                                    <span class="row-name">"Intensity / week (h)"</span>
                                    <input prop:value=move || target_intensity.get().to_string() on:input=move |ev| set_target_intensity.set(event_target_value(&ev).parse().unwrap_or(0.0)) />
                                    <div class="target-progress">
                                        <div class="bar"><div class="bar-fill" style=move || format!("width: {}%", goal_pct(week_intensity_h(), target_intensity.get()))></div></div>
                                        <span class="target-hint">{move || format!("{:.1} h from closed agoges this week", week_intensity_h())}</span>
                                    </div>
                                </div>
                                <button class="btn" on:click=move |_| persist_settings()>"SAVE TARGETS"</button>
                            </div>

                            <hr class="hairline" />

                            <div class="goal-section">
                                <h3 class="section-h3">"BODY GOALS"</h3>
                                <div class="target-row">
                                    <span class="row-name">"Weight (kg)"</span>
                                    <input prop:value=move || target_weight.get().to_string() on:input=move |ev| set_target_weight.set(event_target_value(&ev).parse().unwrap_or(0.0)) />
                                    {move || {
                                        let latest = measurements.get().into_iter().find(|m| m.metric == "weight_kg");
                                        match latest {
                                            Some(m) => view! {
                                                <div class="target-progress">
                                                    <div class="bar"><div class="bar-fill" style=format!("width: {}%", goal_pct(m.value, target_weight.get()))></div></div>
                                                    <span class="target-hint">{format!("latest {:.1} kg · {:.0}% of goal", m.value, goal_pct(m.value, target_weight.get()))}</span>
                                                </div>
                                            }.into_view(),
                                            None => view! {
                                                <div class="target-progress"><span class="target-hint muted">"no weight logged yet — measure below"</span></div>
                                            }.into_view(),
                                        }
                                    }}
                                </div>
                                <div class="target-row">
                                    <span class="row-name">"Body fat (%)"</span>
                                    <input prop:value=move || target_body_fat.get().to_string() on:input=move |ev| set_target_body_fat.set(event_target_value(&ev).parse().unwrap_or(0.0)) />
                                    {move || {
                                        let latest = measurements.get().into_iter().find(|m| m.metric == "body_fat_pct");
                                        match latest {
                                            Some(m) => view! {
                                                <div class="target-progress">
                                                    <div class="bar"><div class="bar-fill" style=format!("width: {}%", goal_pct(m.value, target_body_fat.get()))></div></div>
                                                    <span class="target-hint">{format!("latest {:.1} % · {:.0}% of goal", m.value, goal_pct(m.value, target_body_fat.get()))}</span>
                                                </div>
                                            }.into_view(),
                                            None => view! {
                                                <div class="target-progress"><span class="target-hint muted">"no body-fat logged yet — measure below"</span></div>
                                            }.into_view(),
                                        }
                                    }}
                                </div>
                                <button class="btn small pythia-btn" on:click=oracle_ask_goals><span class="pythia-btn-glyph" inner_html=LAMBDA></span>"PYTHIA — SET GOALS"</button>
                            </div>

                            <hr class="hairline" />

                            <div class="goal-section">
                                <div class="section-head">
                                    <h3 class="section-h3">"MEASUREMENTS"</h3>
                                    <button class="btn small pythia-btn" on:click=oracle_ask_measure><span class="pythia-btn-glyph" inner_html=LAMBDA></span>"PYTHIA — LOG VIA AI"</button>
                                </div>
                                <div class="settings-grid">
                                    <label class="ctl">"WEIGHT (KG)"
                                        <input prop:value=meas_weight on:input=move |ev| set_meas_weight.set(event_target_value(&ev)) placeholder="82.5" />
                                    </label>
                                    <button class="btn" on:click=move |_| log_measurement("weight_kg")>"LOG WEIGHT"</button>
                                    <button class="btn small pythia-btn" on:click=oracle_ask_weight><span class="pythia-btn-glyph" inner_html=LAMBDA></span>"PYTHIA"</button>
                                    <label class="ctl">"BODY FAT (%)"
                                        <input prop:value=meas_body_fat on:input=move |ev| set_meas_body_fat.set(event_target_value(&ev)) placeholder="15.0" />
                                    </label>
                                    <button class="btn" on:click=move |_| log_measurement("body_fat_pct")>"LOG BODY FAT"</button>
                                    <button class="btn small pythia-btn" on:click=oracle_ask_body_fat><span class="pythia-btn-glyph" inner_html=LAMBDA></span>"PYTHIA"</button>
                                </div>
                                <p class="muted" style="margin:10px 0 6px">"latest first · every log also lands in the SKOPOS action log"</p>
                                <ul class="list measure-list">
                                    <For each=move || measurements.get() key=|m| format!("{}-{}", m.metric, m.ts) let:m>
                                        {move || {
                                            let name = if m.metric == "weight_kg" { "WEIGHT".to_string() } else { "BODY FAT".to_string() };
                                            let val = if m.metric == "weight_kg" { format!("{:.1} kg", m.value) } else { format!("{:.1} %", m.value) };
                                            let time = m.ts_ms().map(fmt_time).unwrap_or_default();
                                            view! {
                                                <li class="row">
                                                    <span class="row-name">{name}</span>
                                                    <span class="metric-val">{val}</span>
                                                    <span class="row-time">{time}</span>
                                                </li>
                                            }
                                        }}
                                    </For>
                                </ul>
                                <Show when=move || measurements.get().is_empty() fallback=|| ()>
                                    <p class="muted">"No measurements yet — log your first weight or body-fat above."</p>
                                </Show>
                            </div>
                        </section>
                    }.into_view(),

                    Tab::Enomotia => view! {
                        <TabHero tab=Tab::Enomotia />
                        <section class="panel">
                            <p class="muted" style="margin:20px 0">"The sworn band is coming. Link other accounts to compare and challenge — share your Agoge and stand side by side with your brothers."</p>
                            <p class="muted">"Not yet wired — the backend already scopes every record by user_id, so this is a matter of an invite/join flow, not a data-model change."</p>
                        </section>
                    }.into_view(),

                    Tab::Syssitia => view! {
                        <TabHero tab=Tab::Syssitia />
                        <section class="panel">
                            <div class="panel-head">
                                <h2>"DAILY RATION"</h2>
                                <div class="nut-nav">
                                    <span class="nut-date">{move || { let d = nut_day.get(); if d == today_str() { "TODAY".to_string() } else { d } }}</span>
                                    <button class="btn small" on:click=move |_| nut_day_nav(-1)>"PREV"</button>
                                    <button class="btn small" on:click=move |_| nut_day_nav(1)>"NEXT"</button>
                                    <button class="btn small" on:click=nut_day_today>"TODAY"</button>
                                </div>
                            </div>
                            <div class="kpi">
                                <div class="kpi-chip">
                                    <span class="kpi-label">"KCAL"</span>
                                    <div class="kpi-value">{move || nut_daily.get().map(|d| format!("{:.0}", d.kcal)).unwrap_or_else(|| "—".to_string())}<span class="unit">{move || format!("/ {:.0}", goal_f64(&goal_kcal.get(), 2200.0))}</span></div>
                                </div>
                                <div class="kpi-chip">
                                    <span class="kpi-label">"PROTEIN"</span>
                                    <div class="kpi-value">{move || nut_daily.get().map(|d| format!("{:.0}", d.protein)).unwrap_or_else(|| "—".to_string())}<span class="unit">{move || format!("/ {:.0} G", goal_f64(&goal_protein.get(), 140.0))}</span></div>
                                </div>
                                <div class="kpi-chip">
                                    <span class="kpi-label">"CARBS"</span>
                                    <div class="kpi-value">{move || nut_daily.get().map(|d| format!("{:.0}", d.carbs)).unwrap_or_else(|| "—".to_string())}<span class="unit">{move || format!("/ {:.0} G", goal_f64(&goal_carb.get(), 250.0))}</span></div>
                                </div>
                                <div class="kpi-chip">
                                    <span class="kpi-label">"FAT"</span>
                                    <div class="kpi-value">{move || nut_daily.get().map(|d| format!("{:.0}", d.fat)).unwrap_or_else(|| "—".to_string())}<span class="unit">{move || format!("/ {:.0} G", goal_f64(&goal_fat.get(), 70.0))}</span></div>
                                </div>
                                <div class="kpi-chip">
                                    <span class="kpi-label">"WATER"</span>
                                    <div class="kpi-value">{move || nut_daily.get().map(|d| format!("{:.0}", d.water_ml)).unwrap_or_else(|| "—".to_string())}<span class="unit">{move || format!("/ {:.0} ML", goal_f64(&goal_water.get(), 2500.0))}</span></div>
                                </div>
                            </div>
                            <div class="nut-bars">
                                <div class="nut-row">
                                    <span class="nut-label">"KCAL"</span>
                                    <div class="bar">
                                        <div class="bar-fill" style=move || format!("width: {}%", goal_pct(nut_daily.get().map(|d| d.kcal).unwrap_or(0.0), goal_f64(&goal_kcal.get(), 2200.0)))></div>
                                    </div>
                                    <span class="nut-num">{move || format!("{:.0} / {:.0}", nut_daily.get().map(|d| d.kcal).unwrap_or(0.0), goal_f64(&goal_kcal.get(), 2200.0))}</span>
                                </div>
                                <div class="nut-row">
                                    <span class="nut-label">"PROTEIN"</span>
                                    <div class="bar">
                                        <div class="bar-fill" style=move || format!("width: {}%", goal_pct(nut_daily.get().map(|d| d.protein).unwrap_or(0.0), goal_f64(&goal_protein.get(), 140.0)))></div>
                                    </div>
                                    <span class="nut-num">{move || format!("{:.0} / {:.0}", nut_daily.get().map(|d| d.protein).unwrap_or(0.0), goal_f64(&goal_protein.get(), 140.0))}</span>
                                </div>
                                <div class="nut-row">
                                    <span class="nut-label">"CARBS"</span>
                                    <div class="bar">
                                        <div class="bar-fill" style=move || format!("width: {}%", goal_pct(nut_daily.get().map(|d| d.carbs).unwrap_or(0.0), goal_f64(&goal_carb.get(), 250.0)))></div>
                                    </div>
                                    <span class="nut-num">{move || format!("{:.0} / {:.0}", nut_daily.get().map(|d| d.carbs).unwrap_or(0.0), goal_f64(&goal_carb.get(), 250.0))}</span>
                                </div>
                                <div class="nut-row">
                                    <span class="nut-label">"FAT"</span>
                                    <div class="bar">
                                        <div class="bar-fill" style=move || format!("width: {}%", goal_pct(nut_daily.get().map(|d| d.fat).unwrap_or(0.0), goal_f64(&goal_fat.get(), 70.0)))></div>
                                    </div>
                                    <span class="nut-num">{move || format!("{:.0} / {:.0}", nut_daily.get().map(|d| d.fat).unwrap_or(0.0), goal_f64(&goal_fat.get(), 70.0))}</span>
                                </div>
                                <div class="nut-row">
                                    <span class="nut-label">"WATER"</span>
                                    <div class="bar">
                                        <div class="bar-fill" style=move || format!("width: {}%", goal_pct(nut_daily.get().map(|d| d.water_ml).unwrap_or(0.0), goal_f64(&goal_water.get(), 2500.0)))></div>
                                    </div>
                                    <span class="nut-num">{move || format!("{:.0} / {:.0}", nut_daily.get().map(|d| d.water_ml).unwrap_or(0.0), goal_f64(&goal_water.get(), 2500.0))}</span>
                                </div>
                            </div>
                        </section>
                        <section class="panel">
                            <div class="panel-head">
                                <h2>"LOG FOOD / WATER"</h2>
                                <button class="btn small pythia-btn" on:click=oracle_ask_nutrition><span class="pythia-btn-glyph" inner_html=LAMBDA></span>"PYTHIA — LOG A MEAL"</button>
                            </div>
                            <div class="settings-grid">
                                <label class="ctl">"KIND"
                                    <select on:change=move |ev| set_manual_kind.set(event_target_value(&ev))>
                                        <option value="food" selected>"FOOD (kcal)"</option>
                                        <option value="water">"WATER (ml)"</option>
                                        <option value="meal">"MEAL (kcal + macros)"</option>
                                    </select>
                                </label>
                                <label class="ctl">"AMOUNT"
                                    <input prop:value=manual_amount on:input=move |ev| set_manual_amount.set(event_target_value(&ev)) placeholder="kcal or ml" />
                                </label>
                                <Show when=move || manual_kind.get() != "water">
                                    <label class="ctl">"PROTEIN (g)"
                                        <input prop:value=manual_protein on:input=move |ev| set_manual_protein.set(event_target_value(&ev)) placeholder="0" />
                                    </label>
                                    <label class="ctl">"CARBS (g)"
                                        <input prop:value=manual_carbs on:input=move |ev| set_manual_carbs.set(event_target_value(&ev)) placeholder="0" />
                                    </label>
                                    <label class="ctl">"FAT (g)"
                                        <input prop:value=manual_fat on:input=move |ev| set_manual_fat.set(event_target_value(&ev)) placeholder="0" />
                                    </label>
                                </Show>
                                <Show when=move || manual_kind.get() == "meal">
                                    <label class="ctl">"MEAL TYPE"
                                        <select on:change=move |ev| set_manual_meal_type.set(event_target_value(&ev))>
                                            <option value="breakfast" selected>"BREAKFAST"</option>
                                            <option value="lunch">"LUNCH"</option>
                                            <option value="dinner">"DINNER"</option>
                                            <option value="snack">"SNACK"</option>
                                        </select>
                                    </label>
                                </Show>
                                <label class="ctl">"NOTE (OPTIONAL)"
                                    <input prop:value=manual_note on:input=move |ev| set_manual_note.set(event_target_value(&ev)) placeholder="what it was" />
                                </label>
                                <button class="btn" on:click=add_nutrition_manual>"ADD"</button>
                            </div>
                            <div class="settings-grid" style="margin-top:12px">
                                <label class="ctl">"OR DESCRIBE IT — AI ESTIMATES"
                                    <input prop:value=ai_text on:input=move |ev| set_ai_text.set(event_target_value(&ev)) placeholder="two eggs and a slice of toast" />
                                </label>
                                <button class="btn" on:click=ai_submit>"AI LOG"</button>
                            </div>
                        </section>
                        <section class="panel">
                            <div class="panel-head">
                                <h2>"SYSSITIA LOG"</h2>
                                <span class="muted">{move || nut_daily.get().map(|d| { let w = d.meals.iter().filter(|m| m.r#type == "water").count(); format!("{} food · {} water", d.meals.len() - w, w) }).unwrap_or_default()}</span>
                            </div>
                            <ul class="list">
                                <For each=move || nut_daily.get().map(|d| d.meals.into_iter().rev().collect::<Vec<_>>()).unwrap_or_default() key=|m| m.id.clone() let:m>
                                    {move || {
                                        let label = meal_label(&m);
                                        let time = ms_from_iso(&m.consumed_at).map(fmt_time).unwrap_or_default();
                                        view! {
                                            <li class="row">
                                                <span class="row-name">{label}</span>
                                                <span class="row-time">{time}</span>
                                            </li>
                                        }
                                    }}
                                </For>
                            </ul>
                            {move || if nut_daily.get().map(|d| d.meals.is_empty()).unwrap_or(true) {
                                view! { <p class="muted">"Nothing logged for this day."</p> }.into_view()
                            } else {
                                view! {}.into_view()
                            }}
                        </section>
                        <section class="panel">
                            <div class="panel-head"><h2>"GOALS"</h2><span class="muted">"saved under settings.nutrition"</span></div>
                            <div class="settings-grid">
                                <label class="ctl">"WATER (ml)"
                                    <input prop:value=goal_water on:input=move |ev| set_goal_water.set(event_target_value(&ev)) placeholder="2500" />
                                </label>
                                <label class="ctl">"KCAL / DAY"
                                    <input prop:value=goal_kcal on:input=move |ev| set_goal_kcal.set(event_target_value(&ev)) placeholder="2200" />
                                </label>
                                <label class="ctl">"PROTEIN (g)"
                                    <input prop:value=goal_protein on:input=move |ev| set_goal_protein.set(event_target_value(&ev)) placeholder="140" />
                                </label>
                                <label class="ctl">"CARBS (g)"
                                    <input prop:value=goal_carb on:input=move |ev| set_goal_carb.set(event_target_value(&ev)) placeholder="250" />
                                </label>
                                <label class="ctl">"FAT (g)"
                                    <input prop:value=goal_fat on:input=move |ev| set_goal_fat.set(event_target_value(&ev)) placeholder="70" />
                                </label>
                                <button class="btn" on:click=save_nutrition_goals>"SAVE GOALS"</button>
                            </div>
                        </section>
                    }.into_view(),

                    Tab::Rank => view! {
                        <TabHero tab=Tab::Rank />
                        <section class="panel">
                            <div class="panel-head"><h2>"YOUR STATION"</h2><span class="muted">{move || rank_for(sessions.get().len()).0}</span></div>
                            <p class="muted" style="margin-bottom:12px">{move || rank_for(sessions.get().len()).1}</p>
                            <ul class="rank-ladder">
                                {[("Hoplite", 0usize), ("Paidonomos", 5), ("Mothax", 15), ("Hippeis", 30)].iter().map(|(name, threshold)| {
                                    let name = *name; let threshold = *threshold;
                                    view! {
                                        <li class="rank-row" class:current=move || sessions.get().len().cmp(&threshold).is_ge()>
                                            <span class="row-name">{name}</span>
                                            <span class="row-time">{format!("{threshold}+ workouts")}</span>
                                        </li>
                                    }
                                }).collect_view()}
                            </ul>
                        </section>
                    }.into_view(),

                    Tab::Anapavsis => {
                        const RING_C: f64 = 2.0 * std::f64::consts::PI * 54.0; // arc length, r = 54
                        view! {
                            <TabHero tab=Tab::Anapavsis />
                            <section class="panel">
                                <div class="panel-head">
                                    <h2>"READINESS"</h2>
                                    <span class="muted">"score out of 300 · the 300"</span>
                                </div>
                                <Show when=move || readiness_loading.get() fallback=|| ()>
                                    <div class="muted">"MEASURING READINESS…"</div>
                                </Show>
                                <Show when=move || readiness_error.get().is_some() fallback=|| ()>
                                    {move || format!("READINESS UNAVAILABLE — {}", readiness_error.get().unwrap_or_default())}
                                </Show>
                                <Show
                                    when=move || !readiness_loading.get() && readiness_error.get().is_none() && readiness.get().is_some()
                                    fallback=|| ()
                                >
                                    {move || {
                                        let days = readiness.get().unwrap_or_default();
                                        let today = days.last().cloned();
                                        let score = today.as_ref().map(|t| t.score).unwrap_or(0.0);
                                        let dash = (score / 300.0).clamp(0.0, 1.0) * RING_C;
                                        let resting_hr = today.as_ref().and_then(|t| t.resting_hr);
                                        let hrv = today.as_ref().and_then(|t| t.hrv);
                                        // Last 14 days, oldest → newest, for the mini trend.
                                        let trend: Vec<(String, f64)> = days
                                            .iter()
                                            .rev()
                                            .take(14)
                                            .map(|d| (d.date.clone(), d.score))
                                            .collect::<Vec<_>>()
                                            .into_iter()
                                            .rev()
                                            .collect();
                                        let components = today.as_ref().map(|t| {
                                            t.components.iter().map(|c| {
                                                let gain = c.direction == "+";
                                                let width = (c.value / c.max).clamp(0.0, 1.0) * 100.0;
                                                let sign = if gain { "+" } else { "−" };
                                                let cls = if gain { "readiness-component gain" } else { "readiness-component drain" };
                                                let label = c.label.clone();
                                                let value = c.value;
                                                let max = c.max;
                                                view! {
                                                    <div class=cls>
                                                        <div class="rc-head">
                                                            <span class="rc-label">{label}</span>
                                                            <span class="rc-value">{format!("{value:.0} / {max:.0} {sign}")}</span>
                                                        </div>
                                                        <div class="bar"><div class="bar-fill" style={format!("width:{width:.1}%")} /></div>
                                                    </div>
                                                }
                                            }).collect_view()
                                        });
                                        let today_bar_index = trend.len().saturating_sub(1);
                                        let bars = trend.iter().enumerate().map(|(i, (date, s))| {
                                            let h = ((*s) / 300.0 * 64.0).max(1.0);
                                            let x = i as f64 * 24.0 + 3.0;
                                            let y = 64.0 - h;
                                            let cls = if i == today_bar_index { "trend-bar today" } else { "trend-bar" };
                                            view! {
                                                <rect x={x.to_string()} y={y.to_string()} width="18" height={h.to_string()} class=cls>
                                                    <title>{format!("{date} · {s:.0}")}</title>
                                                </rect>
                                            }
                                        }).collect_view();
                                        let today_date = today.as_ref().map(|t| t.date.clone()).unwrap_or_default();
                                        let resting_hr = resting_hr.map(|v| format!("{v:.0}")).unwrap_or_else(|| "—".to_string());
                                        let hrv = hrv.map(|v| format!("{v:.0}")).unwrap_or_else(|| "—".to_string());
                                        view! {
                                            <div class="readiness-layout">
                                                <div class="readiness-ring-col">
                                                    <div class="readiness-ring">
                                                        <svg viewBox="0 0 120 120" class="ring-svg">
                                                            <circle cx="60" cy="60" r="54" class="ring-track" fill="none" stroke-width="8" />
                                                            <circle cx="60" cy="60" r="54" class="ring-arc" fill="none" stroke-width="8"
                                                                transform="rotate(-90 60 60)" stroke-dasharray=format!("{dash} {}", RING_C) />
                                                        </svg>
                                                        <div class="ring-center">
                                                            <span class="ring-score">{format!("{score:.0}")}</span>
                                                            <span class="ring-max">"OF 300"</span>
                                                        </div>
                                                    </div>
                                                    <div class="ring-date">{format!("TODAY · {today_date}")}</div>
                                                </div>
                                                <div class="readiness-side">
                                                    {components.unwrap_or_else(|| Fragment::new(Vec::new()).into())}
                                                    <div class="kpi" style="grid-template-columns: repeat(2, 1fr)">
                                                        <div class="kpi-chip">
                                                            <span class="kpi-label">"RESTING HR"</span>
                                                            <div class="kpi-value">{resting_hr}<span class="unit">"BPM"</span></div>
                                                        </div>
                                                        <div class="kpi-chip">
                                                            <span class="kpi-label">"HRV"</span>
                                                            <div class="kpi-value">{hrv}</div>
                                                        </div>
                                                    </div>
                                                    <div class="readiness-trend">
                                                        <div class="trend-caption">"LAST 14 DAYS · SCORE / 300"</div>
                                                        <svg viewBox="0 0 336 64" class="trend-svg" preserveAspectRatio="none">{bars}</svg>
                                                    </div>
                                                    <p class="muted">"READINESS = 300 + SLEEP RECHARGE − PONOS DRAIN − ACTIVITY DRAIN, NORMALIZED AGAINST YOUR OWN 90-DAY BASELINES. SLEEP LONG, TRAIN HARD, AND THE RING STAYS NEAR THE 300."</p>
                                                </div>
                                            </div>
                                        }
                                    }}
                                </Show>
                            </section>
                            <section class="panel">
                                <div class="panel-head">
                                    <h2>"BASELINES · TRAILING 90 DAYS"</h2>
                                    <span class="muted">"your own history · p10 / p50 / p90"</span>
                                </div>
                                <Show when=move || baselines.get().is_none() fallback=|| ()>
                                    <div class="muted">"NO BASELINES YET — THE TRAILING 90 DAYS ARE STILL FILLING IN."</div>
                                </Show>
                                <Show when=move || baselines.get().is_some() fallback=|| ()>
                                    {move || {
                                        let b = baselines.get().unwrap();
                                        view! {
                                            <div class="kpi" style="grid-template-columns: repeat(3, 1fr)">
                                                <div class="kpi-chip">
                                                    <span class="kpi-label">"RESTING HR · BPM"</span>
                                                    <div class="kpi-value">{baseline_triple(b.resting_hr.as_ref())}</div>
                                                </div>
                                                <div class="kpi-chip">
                                                    <span class="kpi-label">"PONOS · PTS"</span>
                                                    <div class="kpi-value">{baseline_triple(b.stress.as_ref())}</div>
                                                </div>
                                                <div class="kpi-chip">
                                                    <span class="kpi-label">"DYNAMIS · PTS"</span>
                                                    <div class="kpi-value">{baseline_triple(b.battery.as_ref())}</div>
                                                </div>
                                            </div>
                                        }
                                    }}
                                </Show>
                            </section>
                        }
                    }.into_view(),

                    Tab::Nomoi => view! {
                        <TabHero tab=Tab::Nomoi />
                        <section class="panel">
                            <div class="panel-head"><h2>"API"</h2></div>
                            <div class="settings-grid">
                                <label class="ctl">"API BASE URL"
                                    <input prop:value=base on:input=move |ev| set_base.set(event_target_value(&ev)) spellcheck="false" placeholder="BLANK = SAME-ORIGIN" />
                                </label>
                                <label class="ctl">"TOKEN"
                                    <input prop:value=token on:input=move |ev| set_token.set(event_target_value(&ev)) spellcheck="false" />
                                </label>
                            </div>
                        </section>
                        <section class="panel">
                            <div class="panel-head">
                                <h2>"TIMELINE"</h2>
                                <span class="muted">"the default range the diagrams open with"</span>
                            </div>
                            <div class="range-buttons">
                                {RANGES.iter().map(|(d, label)| {
                                    let d = *d;
                                    view! {
                                        <button class="pill" class:on=move || days.get() == d on:click=move |_| set_range_days(d)>{*label}</button>
                                    }
                                }).collect_view()}
                                <span class="muted">"selecting here also switches the current diagrams"</span>
                            </div>
                        </section>
                        <section class="panel">
                            <div class="panel-head">
                                <h2>"PYTHIA — AI PROVIDER"</h2>
                                <button class="btn small pythia-btn" on:click=oracle_ask_ai_config><span class="pythia-btn-glyph" inner_html=LAMBDA></span>"PYTHIA — CONFIG HELP"</button>
                            </div>
                            <details class="advanced">
                                <summary>"PROVIDER CONFIGURATION (ADVANCED)"</summary>
                                <div class="settings-grid">
                                    <label class="ctl">"PROVIDER"
                                        <select on:change=move |ev| set_ai_provider.set(event_target_value(&ev))>
                                            <option value="llamacpp">"LLAMA.CPP (LOCAL)"</option>
                                            <option value="ollama">"OLLAMA (LOCAL)"</option>
                                            <option value="openai">"OPENAI (REMOTE)"</option>
                                        </select>
                                    </label>
                                    <label class="ctl">"BASE URL (CHAT COMPLETIONS)"
                                        <input prop:value=ai_base on:input=move |ev| set_ai_base.set(event_target_value(&ev)) placeholder=move || ai_placeholder() spellcheck="false" />
                                    </label>
                                    <label class="ctl">"MODEL"
                                        <input prop:value=ai_model on:input=move |ev| set_ai_model.set(event_target_value(&ev)) placeholder="llama3" />
                                    </label>
                                    <label class="ctl">"API KEY"
                                        <input type="password" prop:value=ai_key on:input=move |ev| set_ai_key.set(event_target_value(&ev)) placeholder="(blank for local)" />
                                    </label>
                                </div>
                                <p class="muted" style="margin-top:12px">"Feeds the PYTHIA oracle (the red button, every tab) — local llama.cpp / Ollama or remote OpenAI. Nothing leaves this machine unless you point it there."</p>
                                <div class="ai-provider-actions">
                                    <button class="btn" disabled=move || ai_testing.get() on:click=test_ai_provider>{move || if ai_testing.get() { "TESTING…".to_string() } else { "TEST".to_string() }}</button>
                                    <button class="btn" on:click=save_ai_provider>"SAVE"</button>
                                </div>
                                {move || match ai_test_result.get() {
                                    None => view! {}.into_view(),
                                    Some((ok, msg)) => {
                                        let cls = if ok { "ai-test-result ok" } else { "ai-test-result fail" };
                                        view! {
                                            <p class=cls>{msg}</p>
                                        }.into_view()
                                    }
                                }}
                            </details>
                            <div class="sys-prompt-block">
                                <label class="ctl">"SYSTEM PROMPT — PYTHIA BEHAVIOUR DIRECTIVES"
                                    <textarea rows="7" prop:value=ai_sys_prompt on:input=move |ev| set_ai_sys_prompt.set(event_target_value(&ev)) spellcheck="false" placeholder="(empty = built-in oracle directives only)"></textarea>
                                </label>
                                <p class="muted">"Appended to every PYTHIA prompt so the oracle behaves the way you want. Every save is written to the SKOPOS action log and can be reverted there."</p>
                                <div class="ai-provider-actions">
                                    <button class="btn small" on:click=reset_ai_sys_prompt>"RESET TO DEFAULT"</button>
                                    <button class="btn small" on:click=save_ai_provider>"SAVE"</button>
                                    <span class="muted">{move || format!("{} / 4000 chars", ai_sys_prompt.get().chars().count())}</span>
                                </div>
                            </div>
                        </section>
                        <section class="panel import-panel">
                            <div class="panel-head"><h2>"IMPORT DATA"</h2><span class="muted">{move || import_name.get()}</span></div>
                            <div class="settings-grid">
                                <label class="ctl">"SOURCE"
                                    <select on:change=move |ev| set_import_source.set(event_target_value(&ev))>
                                        <option value="csv">"CSV"</option>
                                        <option value="manual">"JSON"</option>
                                        <option value="health_connect">"HEALTH CONNECT"</option>
                                        <option value="apple_health">"APPLE HEALTH"</option>
                                        <option value="garmin">"GARMIN"</option>
                                        <option value="gpx">"GPX"</option>
                                    </select>
                                </label>
                                <label class="ctl">"FILE (.CSV / .JSON / .GPX)"
                                    <input type="file" accept=".csv,.json,.gpx" on:change=on_import_file />
                                </label>
                                <label class="ctl">"DEVICE ID (OPTIONAL)"
                                    <input prop:value=import_device on:input=move |ev| set_import_device.set(event_target_value(&ev)) placeholder="e.g. pixel-9" />
                                </label>
                            </div>
                            <div class="import-actions">
                                <button class="btn" disabled=move || importing.get() on:click=do_import>{move || if importing.get() { "IMPORTING…".to_string() } else { format!("IMPORT {} SAMPLES", import_buf.get().len()) }}</button>
                                <span class="muted">"PARSED IN THE BROWSER — NOTHING IS POSTED UNTIL YOU PRESS IMPORT"</span>
                            </div>
                            {move || {
                                let count = import_buf.get().len();
                                if count == 0 {
                                    return view! {}.into_view();
                                }
                                let preview = import_preview.get();
                                let parse_errs = import_errors.get();
                                view! {
                                    <div class="import-preview">
                                        <p class="muted" style="margin:12px 0 6px">{format!("{count} SAMPLES PARSED · PREVIEW FIRST 5")}</p>
                                        <ul class="list">
                                            {preview.iter().map(|(ts, m, v)| {
                                                let (ts, m, v) = (ts.clone(), m.clone(), *v);
                                                view! {
                                                    <li class="row">
                                                        <span class="row-name">{format!("{m} = {v}")}</span>
                                                        <span class="row-time">{ts}</span>
                                                    </li>
                                                }
                                            }).collect_view()}
                                        </ul>
                                        {parse_errs.iter().take(20).map(|e| {
                                            let e = e.clone();
                                            view! { <p class="import-warn">{e}</p> }
                                        }).collect_view()}
                                    </div>
                                }.into_view()
                            }}
                            {move || match import_result.get() {
                                None => view! {}.into_view(),
                                Some((ins, skip, errs)) => view! {
                                    <div class="import-result">
                                        <span class="import-stat ok">{format!("INSERTED {ins}")}</span>
                                        <span class="import-stat">{format!("SKIPPED {skip}")}</span>
                                    </div>
                                    {errs.iter().take(20).map(|e| {
                                        let e = e.clone();
                                        view! { <p class="import-warn">{e}</p> }
                                    }).collect_view()}
                                }.into_view()
                            }}
                        </section>
                    }.into_view(),
                    Tab::Skopos => view! {
                        <TabHero tab=Tab::Skopos />
                        <section class="panel">
                            <div class="panel-head">
                                <h2>"ACTION LOG"</h2>
                                <span class="muted">{move || format!("{} actions", actions.get().len())}</span>
                            </div>
                            <ul class="list action-list">
                                <For each=move || actions.get() key=|a| format!("{}-{}", a.id, a.reverted_at.is_some()) let:a>
                                    {move || {
                                        let id = a.id.clone();
                                        let reverted = a.reverted_at.is_some();
                                        let time = ms_from_iso(&a.created_at).map(fmt_time).unwrap_or_else(|| a.created_at.chars().take(16).collect());
                                        let target = if a.target.is_empty() { a.kind.clone() } else { a.target.clone() };
                                        view! {
                                            <li class="row action-row">
                                                <span class="action-kind">{a.kind.to_uppercase()}</span>
                                                <span class="action-target">{target}</span>
                                                <span class="row-time">{time}</span>
                                                {if reverted {
                                                    view! { <span class="action-reverted">"REVERTED"</span> }.into_view()
                                                } else {
                                                    view! { <button class="btn small" on:click=move |_| revert_action_fn(id.clone())>"REVERT"</button> }.into_view()
                                                }}
                                            </li>
                                        }
                                    }}
                                </For>
                            </ul>
                            <Show when=move || actions.get().is_empty() fallback=|| ()>
                                <p class="muted">"No persisted actions yet — settings saves, nutrition, and measurements are recorded here and can be reverted."</p>
                            </Show>
                        </section>
                        <section class="panel">
                            <div class="panel-head">
                                <h2>"SKOPOS — ACTIVITY LOG"</h2>
                                <span class="muted">{move || format!("{} events", log.get().len())}</span>
                            </div>
                            <ul class="list log-list">
                                <For each=move || log.get() key=|e| e.id let:e>
                                    <li class="row log-row">
                                        <span class="log-kind">{e.kind.to_uppercase()}</span>
                                        <span class="log-msg">{e.msg.clone()}</span>
                                        <span class="row-time">{fmt_time(e.ts)}</span>
                                    </li>
                                </For>
                            </ul>
                        </section>
                    }.into_view(),
                }
            }
            </main>

            <footer>
                <div class="meander-rule"></div>
                <div class="footer-line">
                    <span class="footer-mark" inner_html=LAMBDA></span>
                    <span>"EPHORIX · RAW METRICS ARE SACRED · SESSIONS ARE DISCIPLINE"</span>
                </div>
                <div class="footer-motto">"ΜΟΛΩΝ ΛΑΒΕ"</div>
                {move || version_line.get().map(|v| view! { <div class="footer-version">{v}</div> })}
            </footer>

            <button class="oracle-fab" on:click=oracle_toggle title="Ask the Pythia oracle about your training">
                <span class="oracle-fab-glyph" inner_html=glyph_svg("shield")></span>
                <span>"PYTHIA"</span>
            </button>
            <Show when=move || oracle_open.get() fallback=|| ()>
                <div class="oracle-panel">
                    <div class="oracle-head">
                        <span class="oracle-head-glyph" inner_html=glyph_svg("shield")></span>
                        <span class="oracle-title">"PYTHIA — ORACLE"</span>
                        <button class="oracle-close" on:click=move |_| set_oracle_open.set(false)>"✕"</button>
                    </div>
                    <div class="oracle-digest">
                        {move || {
                            let d = oracle_digest.get();
                            d.into_iter()
                                .map(|(l, v)| {
                                    view! {
                                        <span class="oracle-digest-chip"><b>{l}</b><span>{v}</span></span>
                                    }
                                })
                                .collect_view()
                        }}
                    </div>
                    <div class="oracle-thread" node_ref=oracle_thread_ref>
                        <For each=move || oracle_msgs.get() key=|m| m.id let:m>
                            {move || {
                                let cls = if m.role == "user" { "oracle-msg user" } else { "oracle-msg assistant" };
                                view! {
                                    <div class=cls>{m.content.clone()}</div>
                                }
                            }}
                        </For>
                        <Show when=move || oracle_busy.get() fallback=|| ()>
                            <div class="oracle-msg assistant"><span class="oracle-think">"THE ORACLE CONSIDERS…"</span></div>
                        </Show>
                    </div>
                    <Show when=move || !oracle_proposals.get().is_empty() fallback=|| ()>
                        <div class="oracle-proposals">
                            <div class="oracle-proposals-head">"PROPOSED CHANGES"</div>
                            <For each=move || oracle_proposals.get() key=|p| p.key.clone() let:p>
                                {move || {
                                    let key = p.key.clone();
                                    let key_checked = key.clone();
                                    let key_change = key.clone();
                                    let key_new = key.clone();
                                    let key_view = key.clone();
                                    view! {
                                        <div class="oracle-proposal">
                                            <label class="oracle-prop-head">
                                                <input type="checkbox"
                                                    checked=move || oracle_proposals.get().iter().find(|r| r.key == key_checked).map(|r| r.checked).unwrap_or(false)
                                                    on:change=move |ev| {
                                                        let c = event_target_checked(&ev);
                                                        set_oracle_proposals.update(|xs| {
                                                            if let Some(r) = xs.iter_mut().find(|r| r.key == key_change) {
                                                                r.checked = c;
                                                            }
                                                        });
                                                    }/>
                                                <span>{p.label.clone()}</span>
                                            </label>
                                            <div class="oracle-prop-diff">
                                                <span class="oracle-prop-old">{p.current.clone()}</span>
                                                <span class="oracle-prop-arrow">"→"</span>
                                                <span class="oracle-prop-new">{move || oracle_proposals.get().iter().find(|r| r.key == key_new).map(|r| r.draft.clone()).unwrap_or_default()}</span>
                                            </div>
                                            <div class="oracle-prop-reason">{p.reason.clone()}</div>
                                            {move || {
                                                let key = key_view.clone();
                                                let key_num_val = key.clone();
                                                let key_num_in = key.clone();
                                                let key_days_val = key.clone();
                                                let key_days_ch = key.clone();
                                                let key_bool_chk = key.clone();
                                                let key_bool_ch = key.clone();
                                                let key_text_val = key.clone();
                                                let key_text_in = key.clone();
                                                let Some(kind) = oracle_proposals.get().into_iter().find(|r| r.key == key).map(|r| r.input) else {
                                                    return view! {}.into_view();
                                                };
                                                match kind {
                                                    ProposalInput::Number => view! {
                                                        <input class="oracle-prop-input" type="number" step="any"
                                                            prop:value=move || oracle_proposals.get().iter().find(|r| r.key == key_num_val).map(|r| r.draft.clone()).unwrap_or_default()
                                                            on:input=move |ev| set_oracle_proposals.update(|xs| {
                                                                if let Some(r) = xs.iter_mut().find(|r| r.key == key_num_in) {
                                                                    r.draft = event_target_value(&ev);
                                                                }
                                                            }) />
                                                    }.into_view(),
                                                    ProposalInput::Days => view! {
                                                        <select class="oracle-prop-input"
                                                            prop:value=move || oracle_proposals.get().iter().find(|r| r.key == key_days_val).map(|r| r.draft.clone()).unwrap_or_default()
                                                            on:change=move |ev| set_oracle_proposals.update(|xs| {
                                                                if let Some(r) = xs.iter_mut().find(|r| r.key == key_days_ch) {
                                                                    r.draft = event_target_value(&ev);
                                                                }
                                                            })>
                                                            <option value="1">"1D"</option>
                                                            <option value="7">"7D"</option>
                                                            <option value="30">"30D"</option>
                                                            <option value="365">"365D"</option>
                                                        </select>
                                                    }.into_view(),
                                                    ProposalInput::Bool => view! {
                                                        <label class="oracle-prop-bool">
                                                            <input type="checkbox"
                                                                checked=move || oracle_proposals.get().iter().find(|r| r.key == key_bool_chk).map(|r| r.draft == "true").unwrap_or(false)
                                                                on:change=move |ev| {
                                                                    let c = event_target_checked(&ev);
                                                                    set_oracle_proposals.update(|xs| {
                                                                        if let Some(r) = xs.iter_mut().find(|r| r.key == key_bool_ch) {
                                                                            r.draft = c.to_string();
                                                                        }
                                                                    });
                                                                }/>
                                                            "ENABLED"
                                                        </label>
                                                    }.into_view(),
                                                    ProposalInput::Text => view! {
                                                        <input class="oracle-prop-input"
                                                            prop:value=move || oracle_proposals.get().iter().find(|r| r.key == key_text_val).map(|r| r.draft.clone()).unwrap_or_default()
                                                            on:input=move |ev| set_oracle_proposals.update(|xs| {
                                                                if let Some(r) = xs.iter_mut().find(|r| r.key == key_text_in) {
                                                                    r.draft = event_target_value(&ev);
                                                                }
                                                            }) />
                                                    }.into_view(),
                                                }
                                            }}
                                        </div>
                                    }
                                }}
                            </For>
                            <div class="oracle-prop-actions">
                                <button class="btn small" on:click=oracle_accept>"ACCEPT SELECTED"</button>
                                <button class="btn small" on:click=oracle_dismiss>"DISMISS"</button>
                            </div>
                        </div>
                    </Show>
                    <Show when=move || oracle_error.get().is_some() fallback=|| ()>
                        <div class="oracle-error">{move || oracle_error.get().unwrap_or_default()}</div>
                    </Show>
                    {move || {
                        if ai_model.get().trim().is_empty() {
                            view! {
                                <div class="oracle-hint">
                                    <span>"NO AI PROVIDER CONFIGURED — THE ORACLE IS SILENT."</span>
                                    <button class="btn small" on:click=move |_| {
                                        set_current_tab.set(Tab::Nomoi);
                                        set_oracle_open.set(false);
                                    }>"OPEN AI CONFIG"</button>
                                </div>
                            }.into_view()
                        } else {
                            view! {
                                <div class="oracle-input-row">
                                    <input class="oracle-input" placeholder="ask the oracle about your training…" spellcheck="false"
                                        prop:value=oracle_input
                                        on:input=move |ev| set_oracle_input.set(event_target_value(&ev))
                                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                                            if ev.key() == "Enter" && !ev.shift_key() {
                                                ev.prevent_default();
                                                oracle_send();
                                            }
                                        } />
                                    <button class="btn small" disabled=move || oracle_busy.get() on:click=move |_| oracle_send()>"SEND"</button>
                                </div>
                            }.into_view()
                        }
                    }}
                </div>
            </Show>
        </div>
    }
}

// ---------------------------------------------------------------------------
// Nomoi import: client-side parsers for CSV / JSON / GPX exports.
// Each returns (samples, errors); samples are ready for POST /api/v1/import
// (canonical metrics, ISO timestamps, canonical units — docs/import-adapter.md).
// ---------------------------------------------------------------------------

/// Canonical unit for a metric.
fn unit_for_metric(metric: &str) -> &'static str {
    match metric {
        "heart_rate" | "resting_hr" => "bpm",
        "steps" | "reps" => "count",
        "active_calories" | "food_kcal" | "resting_kcal" => "kcal",
        "distance_m" => "m",
        "active_seconds" | "sleep_seconds" | "restful_sleep_seconds" => "s",
        "water_ml" => "ml",
        "protein_g" | "carbs_g" | "fat_g" => "g",
        "hrv" => "ms",
        _ => "au",
    }
}

fn make_sample(ts: &str, metric: &str, value: f64) -> Value {
    json!({ "timestamp": ts, "metric": metric, "value": value, "unit": unit_for_metric(metric) })
}

const IMPORT_PARSE_ERRORS_MAX: usize = 50;

fn push_import_error(errors: &mut Vec<String>, msg: String) {
    if errors.len() < IMPORT_PARSE_ERRORS_MAX {
        errors.push(msg);
    }
}

/// Read a picked file as UTF-8 text. `Blob::arrayBuffer()` is a native
/// promise, so no JS closures need outliving the future.
async fn read_file_text(file: &web_sys::File) -> Result<String, String> {
    use wasm_bindgen::JsCast;
    let blob: &web_sys::Blob = file
        .dyn_ref()
        .ok_or_else(|| "picked file is not a blob".to_string())?;
    let buf = wasm_bindgen_futures::JsFuture::from(blob.array_buffer())
        .await
        .map_err(|e| format!("could not read file: {e:?}"))?;
    let bytes: Vec<u8> = js_sys::Uint8Array::new(&buf).to_vec();
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Files selected on a `<input type="file">` change event.
fn event_target_files(ev: &web_sys::Event) -> Option<Vec<web_sys::File>> {
    use wasm_bindgen::JsCast;
    let target: web_sys::EventTarget = ev.target()?;
    let input: web_sys::HtmlInputElement = target.dyn_into().ok()?;
    let files = input.files()?;
    let mut out = Vec::new();
    for i in 0..files.length() {
        if let Some(f) = files.item(i) {
            out.push(f);
        }
    }
    Some(out)
}

/// CSV header name -> canonical metric (`Some("__ts__")` = the timestamp column).
fn csv_column_metric(col: &str) -> Option<&'static str> {
    Some(match col.trim().to_ascii_lowercase().as_str() {
        "timestamp" | "time" | "date" => "__ts__",
        "heart_rate" | "heartrate" | "bpm" | "hr" => "heart_rate",
        "steps" => "steps",
        "active_calories" | "calories" => "active_calories",
        "distance_m" | "distance" => "distance_m",
        "sleep_seconds" => "sleep_seconds",
        "water_ml" => "water_ml",
        "protein_g" => "protein_g",
        "carbs_g" => "carbs_g",
        "fat_g" => "fat_g",
        "reps" => "reps",
        _ => return None,
    })
}

fn parse_import_csv(text: &str) -> (Vec<Value>, Vec<String>) {
    let mut samples = Vec::new();
    let mut errors = Vec::new();
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let Some(header) = lines.next() else {
        return (samples, errors);
    };
    let cols: Vec<Option<&'static str>> = header.split(',').map(csv_column_metric).collect();
    let ts_idx = cols.iter().position(|m| matches!(m, Some("__ts__")));
    for (n, line) in lines.enumerate() {
        let line_no = n + 2;
        let cells: Vec<&str> = line.split(',').map(str::trim).collect();
        if cells.iter().all(|c| c.is_empty()) {
            continue;
        }
        let Some(tsi) = ts_idx else {
            push_import_error(&mut errors, format!("row {line_no}: no timestamp/time/date column in header"));
            break;
        };
        let ts = cells.get(tsi).copied().unwrap_or("");
        if ms_from_iso(ts).is_none() {
            push_import_error(&mut errors, format!("row {line_no}: bad timestamp '{ts}' — row skipped"));
            continue;
        }
        for (i, metric) in cols.iter().enumerate() {
            let Some(metric) = metric.filter(|m| *m != "__ts__") else {
                continue;
            };
            let cell = cells.get(i).copied().unwrap_or("");
            if cell.is_empty() {
                continue;
            }
            match cell.parse::<f64>() {
                Ok(v) if v.is_finite() => samples.push(make_sample(ts, metric, v)),
                _ => push_import_error(&mut errors, format!("row {line_no}: bad value '{cell}' for {metric}")),
            }
        }
    }
    (samples, errors)
}

/// Health Connect `dataType` suffix -> (canonical metric, value fields to
/// try, most specific first). See docs/import-adapter.md §3.1.
fn hc_metric_for(ty: &str) -> Option<(&'static str, &'static [&'static str])> {
    Some(match ty {
        "heartRate" => ("heart_rate", &["heartRate", "value"]),
        "restingHeartRate" => ("resting_hr", &["restingHeartRate", "value"]),
        "stepCount" => ("steps", &["stepCount", "value"]),
        "distance" => ("distance_m", &["distance", "value"]),
        "caloriesBurned" | "activeCalories" => ("active_calories", &["value"]),
        "totalSleep" | "asleep" => ("sleep_seconds", &["sleepDuration", "value"]),
        "deepAsleep" => ("restful_sleep_seconds", &["sleepDuration", "value"]),
        "waterIntake" => ("water_ml", &["value"]),
        _ => return None,
    })
}

fn push_json_error(errors: &mut Vec<String>, i: usize, msg: &str) {
    if errors.len() < IMPORT_PARSE_ERRORS_MAX {
        errors.push(format!("record {i}: {msg}"));
    }
}

fn parse_import_json(text: &str) -> (Vec<Value>, Vec<String>) {
    let mut samples = Vec::new();
    let mut errors = Vec::new();
    let root: Value = match serde_json::from_str(text.trim()) {
        Ok(v) => v,
        Err(e) => return (samples, vec![format!("invalid JSON: {e}")]),
    };
    let records = if root.is_array() {
        root.as_array()
    } else {
        root.get("samples").and_then(|s| s.as_array())
    };
    let Some(records) = records else {
        return (
            samples,
            vec!["expected a JSON array of records or {\"samples\": [...]}" .to_string()],
        );
    };
    for (i, rec) in records.iter().enumerate() {
        if !rec.is_object() {
            push_json_error(&mut errors, i, "record is not an object");
            continue;
        }
        let ts = rec
            .get("timestamp")
            .or_else(|| rec.get("timeIntervalStart"))
            .and_then(|t| t.as_str())
            .unwrap_or("");
        if ms_from_iso(ts).is_none() {
            push_json_error(&mut errors, i, &format!("missing or bad timestamp '{ts}' — record skipped"));
            continue;
        }
        // Shape 1: {"timestamp","metric","value",...} — already canonical.
        if let Some(metric) = rec.get("metric").and_then(|m| m.as_str()) {
            match rec.get("value").and_then(|v| v.as_f64()) {
                Some(v) if v.is_finite() => {
                    let unit = rec.get("unit").and_then(|u| u.as_str());
                    samples.push(json!({ "timestamp": ts, "metric": metric, "value": v, "unit": unit }));
                }
                Some(_) => push_json_error(&mut errors, i, &format!("non-finite value for '{metric}'")),
                None => push_json_error(&mut errors, i, &format!("no numeric value for '{metric}'")),
            }
            continue;
        }
        // Shape 2: Health Connect records {"timestamp","type","<value field>",...}.
        let Some(ty) = rec.get("type").and_then(|t| t.as_str()) else {
            push_json_error(&mut errors, i, "record has neither 'metric' nor 'type'");
            continue;
        };
        let ty = ty.trim().trim_start_matches("health.dataType:");
        match ty {
            "foodConsumption" => {
                let food = rec.get("value");
                let obj = food.and_then(|v| v.as_object());
                for (field, metric) in [
                    ("calories", "food_kcal"),
                    ("protein", "protein_g"),
                    ("carbs", "carbs_g"),
                    ("fat", "fat_g"),
                ] {
                    let Some(v) = obj.and_then(|o| o.get(field)).and_then(|x| x.as_f64()) else {
                        continue;
                    };
                    if v.is_finite() {
                        samples.push(make_sample(ts, metric, v));
                    }
                }
            }
            // No canonical home — dropped silently, per the adapter doc.
            "lightAsleep" | "exerciseState" => {}
            _ => match hc_metric_for(ty) {
                Some((metric, fields)) => {
                    let value = fields.iter().find_map(|f| rec.get(*f)).and_then(|v| v.as_f64());
                    match value {
                        Some(v) if v.is_finite() => samples.push(make_sample(ts, metric, v)),
                        Some(_) => push_json_error(&mut errors, i, &format!("non-finite value for '{ty}'")),
                        None => {} // field absent: emit nothing, never 0
                    }
                }
                None => push_json_error(&mut errors, i, &format!("unknown type '{ty}'")),
            },
        }
    }
    (samples, errors)
}

/// Attribute value from a small `<trkpt .../>` tag (quoted, optional spaces).
fn gpx_attr<'a>(tag: &'a str, key: &str) -> Option<&'a str> {
    let mut from = 0usize;
    while let Some(rel) = tag[from..].find(key) {
        let start = from + rel;
        let before_ok = start == 0
            || tag[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_whitespace() || c == '/');
        let after = tag[start + key.len()..].trim_start();
        if before_ok {
            if let Some(eq) = after.find('=') {
                let val = after[eq + 1..].trim_start();
                if let Some(q) = val.chars().next() {
                    if q == '"' || q == '\'' {
                        let rest = &val[1..];
                        if let Some(end) = rest.find(q) {
                            return Some(&rest[..end]);
                        }
                    }
                }
            }
        }
        from = start + key.len();
    }
    None
}

fn gpx_child<'a>(body: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let i = body.find(&open)?;
    let rest = &body[i + open.len()..];
    let j = rest.find(&close)?;
    Some(&rest[..j])
}

fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6_371_000.0;
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    2.0 * R * a.sqrt().asin()
}

fn parse_import_gpx(text: &str) -> (Vec<Value>, Vec<String>) {
    let mut samples = Vec::new();
    let mut errors = Vec::new();
    let mut last: Option<(f64, f64)> = None;
    let mut from = 0;
    while let Some(rel) = text[from..].find("<trkpt") {
        let start = from + rel;
        let Some(gt) = text[start..].find('>') else {
            break;
        };
        let tag = &text[start..start + gt];
        from = start + gt + 1;
        if tag.trim_end().ends_with("/>") {
            continue; // self-closing: no <time> child
        }
        let Some(close) = text[from..].find("</trkpt>") else {
            break;
        };
        let body = &text[from..from + close];
        from += close;
        let time = gpx_child(body, "time").map(str::trim).unwrap_or("");
        if time.is_empty() {
            continue; // point without time: skip
        }
        if ms_from_iso(time).is_none() {
            push_import_error(&mut errors, format!("trkpt: bad time '{time}' — point skipped"));
            continue;
        }
        let lat = gpx_attr(tag, "lat").and_then(|s| s.parse::<f64>().ok());
        let lon = gpx_attr(tag, "lon").and_then(|s| s.parse::<f64>().ok());
        if let (Some(lat), Some(lon)) = (lat, lon) {
            if let Some((plat, plon)) = last {
                samples.push(make_sample(time, "distance_m", haversine_m(plat, plon, lat, lon)));
            }
            samples.push(make_sample(time, "active_seconds", 1.0));
            last = Some((lat, lon));
        }
    }
    (samples, errors)
}

/// Select value for the type picker ("" = Undefined).
fn option_value(ev: &web_sys::Event) -> Option<String> {
    let val = event_target_value(ev);
    if val.is_empty() { None } else { Some(val) }
}

/// One session row. Fields are computed eagerly per For iteration; the
/// sessions For re-subscribes to `types` so type CRUD refreshes the rows.
#[component]
fn SessionRow(
    session: AgogeSession,
    types: ReadSignal<Vec<AgogeType>>,
    on_delete: Callback<String>,
) -> impl IntoView {
    let id = session.id.clone();
    let status = session.status.clone();
    let (name, color) = session
        .type_id
        .as_ref()
        .and_then(|tid| types.get().into_iter().find(|t| &t.id == tid))
        .map(|t| (t.name.clone(), t.color_code.clone()))
        .unwrap_or_else(|| ("Undefined".to_string(), "#7B0000".to_string()));
    let time_text = {
        let start = ms_from_iso(&session.start_time)
            .map(fmt_time)
            .unwrap_or_else(|| session.start_time.clone());
        match &session.end_time {
            Some(e) => {
                let end = ms_from_iso(e).map(fmt_time).unwrap_or_else(|| e.clone());
                format!("{start} → {end}")
            }
            None => start,
        }
    };
    view! {
        <li class={if status == "active" { "row open" } else { "row" }}>
            <span class="dot" style=format!("background:{color}")></span>
            <span class="row-name">
                {format!("{} · {}", name, if status == "active" { "OPEN" } else { "CLOSED" })}
            </span>
            <span class="row-time">{time_text}</span>
            <button class="btn small" on:click=move |_| on_delete.call(id.clone())>
                "DELETE"
            </button>
        </li>
    }
}

/// Full-page config for one Agoge (None = new). Re-mounted on selection change
/// via `key`, so its field signals re-initialize from the selected type.
#[component]
fn AgogeConfigForm(
    ty: Option<AgogeType>,
    on_save: Callback<(String, String, String, String, Value)>,
    on_delete: Callback<String>,
) -> impl IntoView {
    let is_new = ty.is_none();
    let existing_id = ty.as_ref().map(|t| t.id.clone());
    let (name, set_name) = create_signal(ty.as_ref().map(|t| t.name.clone()).unwrap_or_default());
    let (color, set_color) = create_signal(ty.as_ref().map(|t| t.color_code.clone()).unwrap_or_else(|| "#E53935".to_string()));
    let (icon, set_icon) = create_signal(ty.as_ref().map(|t| t.icon.clone()).unwrap_or_else(|| GLYPH_KEYS[0].to_string()));
    let initial_category = ty.as_ref().map(|t| t.category.clone()).unwrap_or_else(|| "mixed".to_string());
    let (category, set_category) = create_signal(initial_category.clone());
    // Config object: pre-filled from the loaded type, otherwise defaults for
    // the (initial) category. An empty/null config falls back to defaults too.
    let config = create_rw_signal(
        ty.as_ref()
            .map(|t| t.config.clone())
            .filter(|c| c.as_object().map(|o| !o.is_empty()).unwrap_or(false))
            .unwrap_or_else(|| default_config(&initial_category)),
    );
    view! {
        <div class="config-form">
            <label class="ctl">"NAME"
                <input prop:value=name on:input=move |ev| set_name.set(event_target_value(&ev)) maxlength="40" placeholder="AGOGE NAME" />
            </label>
            <div class="config-row">
                <label class="ctl">"COLOR"
                    <input type="color" prop:value=color on:input=move |ev| set_color.set(event_target_value(&ev)) />
                </label>
                <span class="ctl">"GLYPH"
                    <GlyphPicker value=icon set=set_icon />
                </span>
            </div>
            <span class="ctl">"CLUSTER"</span>
            <div class="cat-switch">
                {[("distance", "DISTANCE"), ("repetitive", "REPETITIVE"), ("dynamic", "DYNAMIC"), ("circuit", "CIRCUIT"), ("recovery", "RECOVERY"), ("mixed", "MIXED")].iter().map(|(val, label)| {
                    let val = *val;
                    let label = *label;
                    view! {
                        <button class="cat-btn" class:on=move || category.get() == val on:click=move |_| {
                            set_category.set(val.to_string());
                            config.set(default_config(val));
                        }>{label}</button>
                    }
                }).collect_view()}
            </div>
            {move || match category.get().as_str() {
                "distance" => view! {
                    <div class="subsettings">
                        {num_field(config, "targetDistanceM", "DISTANCE (M)", false, Some(0), None)}
                        {num_field(config, "paceSecPerKm", "PACE (SEC/KM)", true, Some(0), None)}
                    </div>
                }.into_view(),
                "repetitive" => view! {
                    <div class="subsettings">
                        {num_field(config, "targetReps", "REPS", false, Some(0), None)}
                        {num_field(config, "targetSets", "SETS", false, Some(0), None)}
                        {num_field(config, "restSeconds", "REST (SEC)", false, Some(0), None)}
                    </div>
                }.into_view(),
                "dynamic" => view! {
                    <div class="subsettings">
                        {num_field(config, "targetDurationSec", "DURATION (SEC)", false, Some(0), None)}
                        <label class="ctl">"INTENSITY"
                            <select prop:value=move || config.get().get("intensity").and_then(|v| v.as_str()).unwrap_or("moderate").to_string() on:change=move |ev| {
                                let v = event_target_value(&ev);
                                config.update(|c| { c["intensity"] = json!(v); });
                            }>
                                <option value="low">"LOW"</option>
                                <option value="moderate">"MODERATE"</option>
                                <option value="high">"HIGH"</option>
                            </select>
                        </label>
                    </div>
                }.into_view(),
                "circuit" => view! {
                    <div class="subsettings">
                        {num_field(config, "rounds", "ROUNDS", false, Some(0), None)}
                        {num_field(config, "workSeconds", "WORK (SEC)", false, Some(0), None)}
                        {num_field(config, "restSeconds", "REST (SEC)", false, Some(0), None)}
                    </div>
                }.into_view(),
                "recovery" => view! {
                    <div class="subsettings">
                        {num_field(config, "targetDurationSec", "DURATION (SEC)", false, Some(0), None)}
                        {num_field(config, "maxHrPercent", "MAX HR %", false, Some(0), Some(100))}
                    </div>
                }.into_view(),
                _ => view! {
                    <div class="subsettings">
                        {num_field(config, "targetDurationSec", "DURATION (SEC)", false, Some(0), None)}
                    </div>
                }.into_view(),
            }}
            <div class="config-actions">
                <button class="btn" on:click=move |_| on_save.call((name.get(), color.get(), icon.get(), category.get(), config.get()))>
                    {if is_new { "CREATE" } else { "SAVE" }}
                </button>
                {existing_id.clone().map(|id| {
                    view! {
                        <button class="btn danger" on:click=move |_| on_delete.call(id.clone())>"DELETE"</button>
                    }
                })}
            </div>
        </div>
    }
}

/// Sensible per-category defaults for the free-form `config` object.
fn default_config(category: &str) -> Value {
    match category {
        "distance" => json!({ "targetDistanceM": 0, "paceSecPerKm": null }),
        "repetitive" => json!({ "targetReps": 0, "targetSets": 0, "restSeconds": 0 }),
        "dynamic" => json!({ "targetDurationSec": 0, "intensity": "moderate" }),
        "circuit" => json!({ "rounds": 0, "workSeconds": 0, "restSeconds": 0 }),
        "recovery" => json!({ "targetDurationSec": 0, "maxHrPercent": 0 }),
        _ => json!({ "targetDurationSec": 0 }),
    }
}

/// Labeled integer input bound to a single `config` object key. Empty input
/// resolves to `0` (or `null` for optional fields) on the object.
fn num_field(
    config: RwSignal<Value>,
    key: &'static str,
    label: &'static str,
    optional: bool,
    min: Option<i64>,
    max: Option<i64>,
) -> impl IntoView {
    let value = move || {
        config
            .get()
            .get(key)
            .and_then(|v| v.as_i64())
            .map(|n| n.to_string())
            .unwrap_or_default()
    };
    let on_input = move |ev: web_sys::Event| {
        let trimmed = event_target_value(&ev);
        let trimmed = trimmed.trim();
        let parsed = if trimmed.is_empty() {
            if optional { Value::Null } else { json!(0) }
        } else {
            json!(trimmed.parse::<i64>().unwrap_or(0))
        };
        config.update(|c| {
            c[key] = parsed;
        });
    };
    let min_attr = min.map(|v| v.to_string());
    let max_attr = max.map(|v| v.to_string());
    view! {
        <label class="ctl">
            {label}
            <input type="number" prop:value=value on:input=on_input min=min_attr max=max_attr />
        </label>
    }
}

/// Glyph swatch picker bound to a signal pair holding a glyph key.
#[component]
fn GlyphPicker(value: ReadSignal<String>, set: WriteSignal<String>) -> impl IntoView {
    view! {
        <div class="glyph-picker">
            <For
                each=move || GLYPH_KEYS.to_vec()
                key=|k| k.to_string()
                let:k
            >
                <button
                    class={move || {
                        if value.get() == k {
                            "glyph-btn active"
                        } else {
                            "glyph-btn"
                        }
                    }}
                    title=k
                    on:click=move |_| set.set(k.to_string())
                >
                    <span inner_html=glyph_svg(k)></span>
                </button>
            </For>
        </div>
    }
}

// ---------------------------------------------------------------------------
// Timeline marker interaction helpers + components
// ---------------------------------------------------------------------------

/// "H:MM:SS" for a whole-second count, for the session details panel.
fn hms(total: i64) -> String {
    format!("{}:{:02}:{:02}", total / 3600, (total % 3600) / 60, total % 60)
}

/// Parse an "HH:MM" duration (multi-digit hours ok) into whole seconds.
fn parse_hhmm(s: &str) -> Option<i64> {
    let (h, m) = s.trim().split_once(':')?;
    let h: i64 = h.trim().parse().ok()?;
    let m: i64 = m.trim().parse().ok()?;
    if h < 0 || m < 0 || m > 59 {
        return None;
    }
    Some(h * 3600 + m * 60)
}

/// One session strip chip: colored dot + type name + HH:MM start time.
/// Clicking it opens the session's details panel.
#[component]
fn SessionChip(
    session: AgogeSession,
    types: ReadSignal<Vec<AgogeType>>,
    on_open: Callback<AgogeSession>,
) -> impl IntoView {
    let sid = session.type_id.clone();
    let name = {
        let sid = sid.clone();
        move || {
            sid.as_ref()
                .and_then(|tid| types.get().into_iter().find(|t| &t.id == tid))
                .map(|t| t.name.clone())
                .unwrap_or_else(|| "Undefined".to_string())
        }
    };
    let color = {
        let sid = sid.clone();
        move || {
            sid.as_ref()
                .and_then(|tid| types.get().into_iter().find(|t| &t.id == tid))
                .map(|t| t.color_code.clone())
                .unwrap_or_else(|| "#7B0000".to_string())
        }
    };
    let hhmm = ms_from_iso(&session.start_time)
        .map(|ms| {
            let iso = iso_from_ms(ms);
            if iso.len() >= 16 { iso[11..16].to_string() } else { iso }
        })
        .unwrap_or_default();
    let s = session.clone();
    view! {
        <button class="session-chip" on:click=move |_| on_open.call(s.clone())>
            <span class="dot" style=move || format!("background:{}", color())></span>
            <span class="chip-name">{move || name()}</span>
            <span class="chip-time">{hhmm}</span>
        </button>
    }
}

/// Details-first panel for a selected session: stats up front (loaded from
/// the stats endpoint on open), then type, graphical start/end picking
/// (click the chart), a duration override, and the destructive actions.
#[component]
fn SessionDetails(
    session: AgogeSession,
    base: String,
    token: String,
    types: ReadSignal<Vec<AgogeType>>,
    pick_field: ReadSignal<Option<PickField>>,
    on_type: Callback<(String, String)>,
    on_pick: Callback<PickField>,
    on_duration: Callback<(String, String)>,
    on_delete: Callback<String>,
    on_close: Callback<String>,
) -> impl IntoView {
    let is_active = session.status == "active";
    let id = session.id.clone();
    // Watch stop summary (StopSummaryJson -> agoge_sessions, camelCase on the
    // session object): intensity + distance have no live-computed equivalent
    // in /stats, so they render from the session itself when the watch
    // reported them (None = "—").
    let summary_intensity = session.movement_intensity;
    let summary_distance = session.distance_m;
    // Shared read handles so the per-row closures below can clone the
    // connection strings without moving them out of a multi-call Fn scope.
    let (base_r, _base_w) = create_signal(base.clone());
    let (token_r, _token_w) = create_signal(token.clone());
    let (sid_r, _sid_w) = create_signal(id.clone());
    let (type_id, set_type_id) = create_signal(session.type_id.clone().unwrap_or_default());

    // Type name + swatch, kept live against the type dictionary.
    let sid = session.type_id.clone();
    let name = {
        let sid = sid.clone();
        move || {
            sid.as_ref()
                .and_then(|tid| types.get().into_iter().find(|t| &t.id == tid))
                .map(|t| t.name.clone())
                .unwrap_or_else(|| "Undefined".to_string())
        }
    };
    let color = {
        let sid = sid.clone();
        move || {
            sid.as_ref()
                .and_then(|tid| types.get().into_iter().find(|t| &t.id == tid))
                .map(|t| t.color_code.clone())
                .unwrap_or_else(|| "#7B0000".to_string())
        }
    };

    // Stats: fetched on open (the panel is keyed by session id, so this
    // runs once per selection) and re-fetched after manual-set mutations,
    // so the SETS / TOTAL REPS / VOLUME chips stay current.
    let (stats, set_stats) = create_signal(None::<SessionStats>);
    let (stats_error, set_stats_error) = create_signal(None::<String>);
    let (loading, set_loading) = create_signal(true);

    // Manual exercise sets: loaded with the stats; re-fetched after every
    // mutation (a PATCH rewrites the set rows, changing the exercise ids,
    // so the For below re-keys rows and drafts re-sync from the server).
    let (exercises, set_exercises) = create_signal(None::<Vec<Exercise>>);
    let (ex_error, set_ex_error) = create_signal(None::<String>);
    let (ex_loading, set_ex_loading) = create_signal(true);

    // Re-pull stats + exercises after any manual-set change.
    let refresh_ex = {
        let base = base.clone();
        let token = token.clone();
        let sid = id.clone();
        move || {
            let (b, t, s) = (base.clone(), token.clone(), sid.clone());
            spawn_local(async move {
                match fetch_session_stats(&b, &t, &s).await {
                    Ok(st) => {
                        set_stats.set(Some(st));
                        set_stats_error.set(None);
                    }
                    Err(e) => set_stats_error.set(Some(e)),
                }
                set_loading.set(false);
            });
            let (b, t, s) = (base.clone(), token.clone(), sid.clone());
            spawn_local(async move {
                match fetch_exercises(&b, &t, &s).await {
                    Ok(list) => set_exercises.set(Some(list)),
                    Err(e) => set_ex_error.set(Some(e)),
                }
                set_ex_loading.set(false);
            });
        }
    };
    refresh_ex();
    let on_ex_saved = Callback::new(move |_| refresh_ex());

    // DELETE EXERCISE: rows report the id; the delete + refresh happen
    // here, next to every other session mutation.
    let on_delete_exercise = {
        let base = base.clone();
        let token = token.clone();
        let sid = id.clone();
        let on_saved = on_ex_saved.clone();
        Callback::new(move |eid: String| {
            let (b, t, s) = (base.clone(), token.clone(), sid.clone());
            let saved = on_saved.clone();
            spawn_local(async move {
                let _ = delete_exercise(&b, &t, &s, &eid).await;
                saved.call(());
            });
        })
    };

    // ADD EXERCISE: one initial set (setNumber 1); the rest are edited
    // per row.
    let (new_name, set_new_name) = create_signal(String::new());
    let (new_reps, set_new_reps) = create_signal(String::new());
    let (new_weight, set_new_weight) = create_signal(String::new());
    let (adding, set_adding) = create_signal(false);
    let on_add_exercise = {
        let base = base.clone();
        let token = token.clone();
        let sid = id.clone();
        let on_saved = on_ex_saved.clone();
        move |_| {
            let name = new_name.get().trim().to_string();
            if name.is_empty() {
                return;
            }
            set_adding.set(true);
            let reps = new_reps.get().trim().parse::<i32>().unwrap_or(0);
            let weight = new_weight.get().trim().parse::<f64>().ok();
            let body = json!({
                "name": name,
                "sets": [{ "setNumber": 1, "reps": reps, "weightKg": weight, "restSec": null }],
            });
            let (b, t, s) = (base.clone(), token.clone(), sid.clone());
            let saved = on_saved.clone();
            spawn_local(async move {
                let _ = add_exercise(&b, &t, &s, &body).await;
                set_adding.set(false);
                set_new_name.set(String::new());
                set_new_reps.set(String::new());
                set_new_weight.set(String::new());
                saved.call(());
            });
        }
    };

    let start_label = ms_from_iso(&session.start_time)
        .map(fmt_time)
        .unwrap_or_else(|| session.start_time.clone());
    let end_label = session
        .end_time
        .as_deref()
        .and_then(ms_from_iso)
        .map(fmt_time)
        .unwrap_or_else(|| "OPEN".to_string());

    // DURATION override input ("HH:MM"); APPLY computes end = start + duration.
    let (duration, set_duration) = create_signal(String::new());

    let id_type = id.clone();
    let id_dur = id.clone();
    let id_del = id.clone();
    let id_close = id.clone();
    view! {
        <div class="session-editor">
            <div style="flex:1 1 100%; display:flex; align-items:center; gap:10px;">
                <span class="dot" style=move || format!("background:{}", color())></span>
                <span class="row-name">{move || name()}</span>
                <span class="muted">{format!("START {start_label} · END {end_label}")}</span>
                <Show when=move || pick_field.get().is_some() fallback=|| ()>
                    <span class="muted">"NOW CLICK THE CHART AT THE TARGET TIME"</span>
                </Show>
            </div>
            <Show when=move || loading.get() fallback=|| ()>
                <div class="muted">"LOADING STATS…"</div>
            </Show>
            <Show when=move || stats_error.get().is_some() fallback=|| ()>
                {move || format!("STATS UNAVAILABLE — {}", stats_error.get().unwrap_or_default())}
            </Show>
            <Show when=move || stats.get().is_some() fallback=|| ()>
                {move || {
                    let st = stats.get().unwrap();
                    view! {
                        <div class="kpi" style="flex:1 1 460px; min-width:0;">
                            <div class="kpi-chip">
                                <span class="kpi-label">"TOTAL"</span>
                                <div class="kpi-value">{hms(st.duration_sec)}</div>
                            </div>
                            <div class="kpi-chip">
                                <span class="kpi-label">"IN ACTION"</span>
                                <div class="kpi-value">{hms(st.active_sec)}</div>
                            </div>
                            <div class="kpi-chip">
                                <span class="kpi-label">"PAUSE"</span>
                                <div class="kpi-value">{hms(st.pause_sec)}</div>
                            </div>
                            <div class="kpi-chip">
                                <span class="kpi-label">"REPS"</span>
                                <div class="kpi-value">{format!("{}", st.reps)}</div>
                            </div>
                            <div class="kpi-chip">
                                <span class="kpi-label">"KCAL"</span>
                                <div class="kpi-value">{format!("{:.0}", st.calories)}</div>
                            </div>
                            <div class="kpi-chip">
                                <span class="kpi-label">"AVG HR"</span>
                                <div class="kpi-value">{format!("{:.0}", st.avg_hr)}<span class="unit">"BPM"</span></div>
                            </div>
                            <div class="kpi-chip">
                                <span class="kpi-label">"PEAK HR"</span>
                                <div class="kpi-value">{format!("{}", st.peak_hr)}<span class="unit">"BPM"</span></div>
                            </div>
                            <div class="kpi-chip">
                                <span class="kpi-label">"SETS"</span>
                                <div class="kpi-value">{format!("{}", st.sets)}</div>
                            </div>
                            <div class="kpi-chip">
                                <span class="kpi-label">"TOTAL REPS"</span>
                                <div class="kpi-value">{format!("{}", st.total_reps)}</div>
                            </div>
                            <div class="kpi-chip">
                                <span class="kpi-label">"VOLUME"</span>
                                <div class="kpi-value">{format!("{:.0}", st.volume_kg)}<span class="unit">"KG"</span></div>
                            </div>
                        </div>
                    }
                }}
            </Show>
            <Show when=move || summary_intensity.is_some() || summary_distance.is_some() fallback=|| ()>
                <div class="kpi" style="flex:1 1 460px; min-width:0;">
                    <div class="kpi-chip">
                        <span class="kpi-label">"INTENSITY"</span>
                        <div class="kpi-value">{move || summary_intensity.map(|v| format!("{v:.2}")).unwrap_or_else(|| "—".to_string())}</div>
                    </div>
                    <div class="kpi-chip">
                        <span class="kpi-label">"DISTANCE"</span>
                        <div class="kpi-value">{move || summary_distance.map(|v| format!("{v:.0} m")).unwrap_or_else(|| "—".to_string())}</div>
                    </div>
                </div>
            </Show>
            <div style="flex:1 1 100%; display:flex; flex-direction:column; gap:8px; margin-top:14px; padding-top:14px; border-top:1px solid var(--line);">
                <div style="display:flex; align-items:center; gap:10px; flex-wrap:wrap;">
                    <span class="muted" style="font-size:10px; letter-spacing:0.22em;">"EXERCISES"</span>
                    <Show when=move || ex_loading.get() fallback=|| ()>
                        <span class="muted">"LOADING…"</span>
                    </Show>
                    <Show when=move || ex_error.get().is_some() fallback=|| ()>
                        {move || format!("UNAVAILABLE — {}", ex_error.get().unwrap_or_default())}
                    </Show>
                    <Show when=move || exercises.get().as_ref().is_some_and(Vec::is_empty) fallback=|| ()>
                        <span class="muted">"NO MANUAL SETS YET"</span>
                    </Show>
                </div>
                <For each=move || exercises.get().unwrap_or_default() key=|e| e.id.clone() let:e>
                    {move || {
                        let e = e.clone();
                        let base = base_r.get();
                        let token = token_r.get();
                        let sid = sid_r.get();
                        view! {
                            <ExerciseRow ex=e base=base token=token session_id=sid on_saved=on_ex_saved on_delete=on_delete_exercise />
                        }
                    }}
                </For>
                <div style="display:flex; align-items:flex-end; gap:8px; flex-wrap:wrap;">
                    <label class="ctl">"EXERCISE"
                        <input placeholder="e.g. SQUAT" style="min-width:160px" prop:value=new_name on:input=move |ev| set_new_name.set(event_target_value(&ev)) />
                    </label>
                    <label class="ctl">"REPS"
                        <input placeholder="0" style="min-width:64px" prop:value=new_reps on:input=move |ev| set_new_reps.set(event_target_value(&ev)) />
                    </label>
                    <label class="ctl">"WEIGHT KG"
                        <input placeholder="—" style="min-width:80px" prop:value=new_weight on:input=move |ev| set_new_weight.set(event_target_value(&ev)) />
                    </label>
                    <button class="btn" disabled=move || adding.get() on:click=on_add_exercise>"ADD EXERCISE"</button>
                </div>
            </div>
            <label class="ctl">"TYPE"
                <select prop:value=type_id on:change=move |ev| {
                    let v = event_target_value(&ev);
                    set_type_id.set(v.clone());
                    on_type.call((id_type.clone(), v));
                }>
                    <option value="">"UNDEFINED"</option>
                    <For each=move || types.get() key=|t| t.id.clone() let:t>
                        <option value=t.id.clone()>{t.name.clone()}</option>
                    </For>
                </select>
            </label>
            <button class="btn" class:on=move || pick_field.get() == Some(PickField::Start)
                    on:click=move |_| on_pick.call(PickField::Start)>
                {move || if pick_field.get() == Some(PickField::Start) { "PICKING START…" } else { "SET START" }}
            </button>
            <button class="btn" class:on=move || pick_field.get() == Some(PickField::End)
                    on:click=move |_| on_pick.call(PickField::End)>
                {move || if pick_field.get() == Some(PickField::End) { "PICKING END…" } else { "SET END" }}
            </button>
            <label class="ctl">"DURATION"
                <input placeholder="HH:MM" prop:value=duration on:input=move |ev| set_duration.set(event_target_value(&ev)) />
            </label>
            <button class="btn" on:click=move |_| on_duration.call((id_dur.clone(), duration.get()))>"APPLY"</button>
            <button class="btn danger" on:click=move |_| on_delete.call(id_del.clone())>"DELETE"</button>
            {is_active.then(|| view! {
                <button class="btn" on:click=move |_| on_close.call(id_close.clone())>"CLOSE NOW"</button>
            })}
        </div>
    }
}

/// Which raw input field of a set row to write. Drafts keep strings so
/// the inputs echo exactly what was typed; numbers parse only on SAVE.
#[derive(Clone, Copy)]
enum SetField {
    Reps,
    Weight,
    Rest,
}

/// One draft set: set number plus raw input strings for reps/weight/rest.
#[derive(Debug, Clone, PartialEq)]
struct SetDraft {
    set_number: i32,
    reps: String,
    weight: String,
    rest: String,
}

/// One exercise group with inline per-set editing. The draft is local;
/// SAVE PATCHes the name plus the whole sets array, DISCARD restores
/// the loaded values. The parent re-fetches the list after each
/// mutation, so this row (keyed by exercise id) always re-syncs from
/// the server and can never hold a stale draft.
#[component]
fn ExerciseRow(
    ex: Exercise,
    base: String,
    token: String,
    session_id: String,
    on_saved: Callback<()>,
    on_delete: Callback<String>,
) -> impl IntoView {
    let to_draft = |s: &ExerciseSet| SetDraft {
        set_number: s.set_number,
        reps: s.reps.to_string(),
        weight: s.weight_kg.map(|w| format!("{w}")).unwrap_or_default(),
        rest: s.rest_sec.map(|r| r.to_string()).unwrap_or_default(),
    };
    let orig_name = ex.name.clone();
    let orig_sets: Vec<SetDraft> = ex.sets.iter().map(to_draft).collect();
    let (name, set_name) = create_signal(ex.name.clone());
    let (sets, set_sets) = create_signal(ex.sets.iter().map(to_draft).collect::<Vec<_>>());
    let (dirty, set_dirty) = create_signal(false);
    let (saving, set_saving) = create_signal(false);
    let (save_error, set_save_error) = create_signal(None::<String>);

    // Write one raw field of one set, looked up by set number. Kept as a
    // Callback (Copy) so each per-set input handler captures its own copy;
    // a bare closure value could only be moved into one of them.
    let patch_set = Callback::new(move |(num, field, v): (i32, SetField, String)| {
        set_save_error.set(None);
        set_sets.update(|sets| {
            if let Some(s) = sets.iter_mut().find(|s| s.set_number == num) {
                match field {
                    SetField::Reps => s.reps = v,
                    SetField::Weight => s.weight = v,
                    SetField::Rest => s.rest = v,
                }
            }
        });
        set_dirty.set(true);
    });

    let on_name = move |ev: web_sys::Event| {
        set_save_error.set(None);
        set_name.set(event_target_value(&ev));
        set_dirty.set(true);
    };

    // Append an empty set numbered max+1 (draft; persisted with the row).
    let on_add_set = move |_| {
        set_save_error.set(None);
        set_sets.update(|sets| {
            let next = sets.iter().map(|s| s.set_number).max().unwrap_or(0) + 1;
            sets.push(SetDraft {
                set_number: next,
                reps: String::new(),
                weight: String::new(),
                rest: String::new(),
            });
        });
        set_dirty.set(true);
    };

    let on_delete_set = Callback::new(move |num: i32| {
        set_save_error.set(None);
        set_sets.update(|sets| sets.retain(|s| s.set_number != num));
        set_dirty.set(true);
    });

    let orig_name = orig_name.clone();
    let orig_sets = orig_sets.clone();
    let on_discard = Callback::new(move |_: ()| {
        set_name.set(orig_name.clone());
        set_sets.set(orig_sets.clone());
        set_dirty.set(false);
        set_save_error.set(None);
    });

    let eid_save = ex.id.clone();
    let eid_del = ex.id.clone();
    let base = base.clone();
    let token = token.clone();
    let session_id = session_id.clone();
    let on_save = move |_| {
        set_dirty.set(false);
        set_saving.set(true);
        let name = name.get();
        let sets_json: Vec<Value> = sets
            .get()
            .iter()
            .map(|s| {
                json!({
                    "setNumber": s.set_number,
                    "reps": s.reps.trim().parse::<i32>().unwrap_or(0),
                    "weightKg": s.weight.trim().parse::<f64>().ok(),
                    "restSec": s.rest.trim().parse::<i32>().ok(),
                })
            })
            .collect();
        let body = json!({ "name": name, "sets": sets_json });
        let (b, t, s, e) = (base.clone(), token.clone(), session_id.clone(), eid_save.clone());
        spawn_local(async move {
            let res = update_exercise(&b, &t, &s, &e, &body).await;
            set_saving.set(false);
            match res {
                Ok(_) => on_saved.call(()),
                Err(e) => set_save_error.set(Some(e)),
            }
        });
    };

    let on_exercise_delete = move |_| on_delete.call(eid_del.clone());

    view! {
        <div style="display:flex; flex-direction:column; gap:6px; border:1px solid var(--line); background:var(--panel); padding:10px 12px;">
            <div style="display:flex; align-items:center; gap:8px; flex-wrap:wrap;">
                <input placeholder="EXERCISE" style="flex:1; min-width:140px" prop:value=name on:input=on_name />
                <Show when=move || dirty.get() fallback=|| ()>
                    <span class="muted">"● UNSAVED"</span>
                </Show>
                <button class="btn small" disabled=move || saving.get() || !dirty.get() on:click=on_save>
                    {move || if saving.get() { "SAVING…" } else { "SAVE" }}
                </button>
                <Show when=move || dirty.get() fallback=|| ()>
                    <button class="btn small" on:click=move |_| on_discard.call(())>"DISCARD"</button>
                </Show>
                <button class="btn small" on:click=on_add_set>"ADD SET"</button>
                <button class="btn small danger" on:click=on_exercise_delete>"DELETE"</button>
            </div>
            <For each=move || sets.get() key=|s| s.set_number let:s>
                {move || {
                    let num = s.set_number;
                    view! {
                        <div style="display:flex; align-items:center; gap:6px; flex-wrap:wrap;">
                            <span class="muted" style="min-width:48px;">{format!("SET {num}")}</span>
                            <input style="width:64px; min-width:0;" placeholder="REPS"
                                   prop:value=move || sets.get().iter().find(|x| x.set_number == num).map(|x| x.reps.clone()).unwrap_or_default()
                                   on:input=move |ev| patch_set.call((num, SetField::Reps, event_target_value(&ev))) />
                            <span class="muted">"×"</span>
                            <input style="width:76px; min-width:0;" placeholder="KG"
                                   prop:value=move || sets.get().iter().find(|x| x.set_number == num).map(|x| x.weight.clone()).unwrap_or_default()
                                   on:input=move |ev| patch_set.call((num, SetField::Weight, event_target_value(&ev))) />
                            <span class="muted">"KG"</span>
                            <span class="muted">"REST"</span>
                            <input style="width:64px; min-width:0;" placeholder="S"
                                   prop:value=move || sets.get().iter().find(|x| x.set_number == num).map(|x| x.rest.clone()).unwrap_or_default()
                                   on:input=move |ev| patch_set.call((num, SetField::Rest, event_target_value(&ev))) />
                            <span class="muted">"S"</span>
                            <Show when=move || (sets.get().len() > 1) fallback=|| ()>
                                <button class="btn small danger" title="DELETE SET" on:click=move |_| on_delete_set.call(num)>{"✕"}</button>
                            </Show>
                        </div>
                    }
                }}
            </For>
            <Show when=move || save_error.get().is_some() fallback=|| ()>
                {move || format!("SAVE FAILED — {}", save_error.get().unwrap_or_default())}
            </Show>
        </div>
    }
}
