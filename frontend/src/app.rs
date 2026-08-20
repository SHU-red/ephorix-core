//! Root application: header, timeline, sessions, types. Black/red Spartan UI.
//! UI preferences (series visibility, range) are persisted in the backend
//! DB (`user_settings`) — the web app needs no second volume.

use leptos::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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

/// Time-range presets for the timeline buttons (label, days).
const RANGES: &[(i64, &str)] = &[(1, "1D"), (7, "1W"), (30, "1M"), (365, "1Y")];

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
            Tab::Anapavsis => "Rest, refreshment, recovery. Your body battery: how much the night restored and how much the day's askesis drained.",
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
    let (cursor, set_cursor) = create_signal(None::<f64>);
    let (error, set_error) = create_signal(None::<String>);
    let (selected_type, set_selected_type) = create_signal(None::<String>);
    let (series, set_series) = create_signal(SeriesConfig::default());
    let (settings_loaded, set_settings_loaded) = create_signal(false);
    let clear_sel = create_rw_signal(0u32);
    let reset_zoom = create_rw_signal(0u32);
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
    let (body_energy, set_body_energy) = create_signal(None::<BodyEnergyDay>);
    let (battery_series, set_battery_series) = create_signal(Vec::<BatterySeriesPoint>::new());
    let (log, set_log) = create_signal(Vec::<LogEntry>::new());
    let log_counter = create_rw_signal(0u64);
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
    // Leonidas targets (persisted in settings).
    let (target_steps, set_target_steps) = create_signal(10_000i64);
    let (target_kcal, set_target_kcal) = create_signal(500i64);
    let (target_sleep, set_target_sleep) = create_signal(8.0f64);
    // Syssitia manual entry.
    let (manual_kind, set_manual_kind) = create_signal("food".to_string());
    let (manual_amount, set_manual_amount) = create_signal(String::new());

    // Persist series visibility + range to the backend settings.
    let persist_settings = move || {
        let base = base.get();
        let token = token.get();
        let s = series.get();
        let d = days.get();
        let ai = json!({ "baseUrl": ai_base.get(), "model": ai_model.get(), "apiKey": ai_key.get() });
        let targets = json!({ "steps": target_steps.get(), "kcal": target_kcal.get(), "sleepH": target_sleep.get() });
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
                Ok(tt) => {
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
                        set_ai_base.set(ai.get("baseUrl").and_then(|v| v.as_str()).unwrap_or("").to_string());
                        set_ai_model.set(ai.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string());
                        set_ai_key.set(ai.get("apiKey").and_then(|v| v.as_str()).unwrap_or("").to_string());
                    }
                    if let Some(t) = sv.get("targets") {
                        set_target_steps.set(t.get("steps").and_then(|v| v.as_i64()).unwrap_or(10_000));
                        set_target_kcal.set(t.get("kcal").and_then(|v| v.as_i64()).unwrap_or(500));
                        set_target_sleep.set(t.get("sleepH").and_then(|v| v.as_f64()).unwrap_or(8.0));
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

    let clear_selection = move |_| {
        clear_sel.set(clear_sel.get() + 1);
        set_selection.set(None);
    };

    let close_open_at_cursor = move |_| {
        let Some(ts) = cursor.get() else {
            set_error.set(Some("move the cursor over the timeline first".to_string()));
            return;
        };
        let Some(open) = sessions.get().into_iter().find(|s| s.status == "active") else {
            set_error.set(Some("no open agoge session".to_string()));
            return;
        };
        let base = base.get();
        let token = token.get();
        let end = iso_from_ms(ts);
        let url = format!("/api/v1/agoge-sessions/{}", open.id);
        spawn_local(async move {
            match patch_json(&base, &token, &url, &json!({ "endTime": end })).await {
                Ok(_) => {
                    // Keep the marker event stream complete for retro-analysis.
                    let _ = post_json(
                        &base,
                        &token,
                        "/api/v1/events/marker",
                        &json!({
                            "kind": "stop",
                            "sessionId": open.id,
                            "occurredAt": end,
                            "source": "web"
                        }),
                    )
                    .await;
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
    // start/end on the selected session; otherwise it creates an open session
    // at that ts.
    let handle_chart_click = move |ts: f64| {
        if let Some(field) = pick_field.get() {
            let Some(s) = selected_session.get() else {
                end_pick();
                return;
            };
            let base = base.get();
            let token = token.get();
            let body = match field {
                PickField::Start => json!({ "startTime": iso_from_ms(ts) }),
                PickField::End => json!({ "endTime": iso_from_ms(ts) }),
            };
            let id = s.id.clone();
            end_pick();
            spawn_local(async move {
                match patch_json(&base, &token, &format!("/api/v1/agoge-sessions/{id}"), &body).await {
                    Ok(_) => {
                        selected_session.set(None);
                        refresh();
                    }
                    Err(e) => set_error.set(Some(e)),
                }
            });
            return;
        }
        let Some(type_id) = selected_type.get() else {
            set_error.set(Some("select a type first".to_string()));
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
                Ok(_) => refresh(),
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
                    if let Err(e) = post_json(&base, &token, "/api/v1/nutrition", &body).await {
                        set_error.set(Some(e));
                    }
                    set_ai_text.set(String::new());
                    log_event("ai", &format!("logged nutrition via AI ({kind} {amount:.0})"));
                    refresh();
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
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            let body = json!({
                "kind": kind,
                "amount": amount,
                "consumedAt": iso_from_ms(js_sys::Date::now()),
            });
            if let Err(e) = post_json(&base, &token, "/api/v1/nutrition", &body).await {
                set_error.set(Some(e));
            }
            set_manual_amount.set(String::new());
            log_event("nutrition", &format!("logged {kind} {amount:.0}"));
            refresh();
        });
    };

    let toggle_zoom_mode = move |_| set_zoom_mode.update(|v| *v = !*v);
    let do_reset_zoom = move |_| reset_zoom.set(reset_zoom.get() + 1);
    // -----------------------------------------------------------------------
    view! {
        <div class="app">
            <header class="header">
                <div class="brand">
                    <div class="brand-row">
                        <h1>
                            <span class="wm-state wm-latin">"EPHORIX"</span>
                            <span class="wm-state wm-greek">"ΕΦΟΡΙΞ"</span>
                        </h1>
                    </div>
                    <span class="sub">"ΑΓΩΓΗ · TRAINING COMMAND"</span>
                </div>
                <img class="brand-helmet" src="/assets/spartan_banner.png" alt="Spartan banner" />
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
                        <div class="kpi">
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
                                <span class="kpi-label">"BODY BATTERY"</span>
                                <div class="kpi-value">
                                    {move || body_energy.get().map(|b| format!("{:.0}", b.score)).unwrap_or_else(|| "—".to_string())}
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
                                            <button class="pill" class:on=move || days.get() == d on:click=move |_| set_range_days(d)>{*label}</button>
                                        }
                                    }).collect_view()}
                                    <span class="muted cursor-readout">
                                        "CURSOR "
                                        {move || cursor.get().map(fmt_time).unwrap_or_else(|| "—".to_string())}
                                    </span>
                                </div>
                            </div>
                            <TimelineChart
                                points=points
                                sessions=sessions
                                types=types
                                nutrition=nutrition
                                sleep=sleep
                                battery=battery_series
                                series=series
                                selection=set_selection
                                cursor=set_cursor
                                pick_mode=pick_mode
                                zoom_mode=zoom_mode
                                clear_trigger=clear_sel
                                reset_zoom=reset_zoom
                                on_click_at=Callback::new(move |ts| handle_chart_click(ts))
                                on_ready=Callback::new(move |id| handle_chart_ready(id))
                                on_session_click=Callback::new(move |sid| handle_session_click(sid))
                            />
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
                            <div class="timeline-actions">
                                <button class="btn" class:on=move || !zoom_mode.get() on:click=toggle_zoom_mode>
                                    {move || if zoom_mode.get() { "DRAG = ZOOM" } else { "DRAG = SELECT" }}
                                </button>
                                <button class="btn" on:click=do_reset_zoom>"RESET ZOOM"</button>
                                <button class="btn" on:click=create_from_selection>"CREATE SESSION FROM SELECTION"</button>
                                <button class="btn" on:click=clear_selection>"CLEAR SELECTION"</button>
                                <button class="btn" on:click=close_open_at_cursor>"CLOSE OPEN AT CURSOR"</button>
                                <label class="ctl">
                                    "TYPE"
                                    <select on:change=move |ev| set_selected_type.set(option_value(&ev))>
                                        <option value="">"UNDEFINED"</option>
                                        <For each=move || types.get() key=|t| t.id.clone() let:t>
                                            <option value=t.id.clone()>{t.name.clone()}</option>
                                        </For>
                                    </select>
                                </label>
                                <span class="muted selection-readout">
                                    "SELECTION: "
                                    {move || selection.get().map(|(f, t)| format!("{} → {}", fmt_time(f.min(t)), fmt_time(f.max(t)))).unwrap_or_else(|| "—".to_string())}
                                </span>
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
                            </div>
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
                            <div class="panel-head"><h2>"TRAINING TARGETS"</h2><span class="muted">"be like Leonidas"</span></div>
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
                            <button class="btn" style="margin-top:12px" on:click=move |_| persist_settings()>"SAVE TARGETS"</button>
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
                            <div class="panel-head"><h2>"LOG FOOD / WATER"</h2></div>
                            <div class="settings-grid">
                                <label class="ctl">"KIND"
                                    <select on:change=move |ev| set_manual_kind.set(event_target_value(&ev))>
                                        <option value="food" selected>"FOOD (kcal)"</option>
                                        <option value="water">"WATER (ml)"</option>
                                    </select>
                                </label>
                                <label class="ctl">"AMOUNT"
                                    <input prop:value=manual_amount on:input=move |ev| set_manual_amount.set(event_target_value(&ev)) placeholder="e.g. 420" />
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
                            <div class="panel-head"><h2>"SYSSITIA LOG"</h2><span class="muted">{move || { let w = nutrition.get().iter().filter(|n| n.kind == "water").count(); let f = nutrition.get().len() - w; format!("{f} food · {w} water") }}</span></div>
                            <ul class="list">
                                <For each=move || nutrition.get().into_iter().rev() key=|n| n.ts.to_string() let:n>
                                    <li class="row">
                                        <span class="row-name">{move || if n.kind == "water" { format!("Water — {:.0} ml", n.amount) } else { format!("Food — {:.0} kcal", n.amount) }}</span>
                                        <span class="row-time">{move || fmt_time(n.ts)}</span>
                                    </li>
                                </For>
                            </ul>
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

                    Tab::Anapavsis => view! {
                        <TabHero tab=Tab::Anapavsis />
                        <section class="panel">
                            <div class="panel-head"><h2>"BODY BATTERY"</h2><span class="muted">"full = 300 · the 300"</span></div>
                            <div class="kpi">
                                <div class="kpi-chip">
                                    <span class="kpi-label">"BODY BATTERY"</span>
                                    <div class="kpi-value">{move || body_energy.get().map(|b| format!("{:.0}", b.score)).unwrap_or_else(|| "—".to_string())}<span class="unit">"/300"</span></div>
                                </div>
                                <div class="kpi-chip">
                                    <span class="kpi-label">"STRESS"</span>
                                    <div class="kpi-value">{move || body_energy.get().map(|b| format!("{:.0}", b.stress)).unwrap_or_else(|| "—".to_string())}<span class="unit">"/100"</span></div>
                                </div>
                                <div class="kpi-chip">
                                    <span class="kpi-label">"DRAIN"</span>
                                    <div class="kpi-value">{move || body_energy.get().map(|b| format!("{:.0}", b.drain)).unwrap_or_else(|| "—".to_string())}<span class="unit">"PTS"</span></div>
                                </div>
                                <div class="kpi-chip">
                                    <span class="kpi-label">"SLEEP (RANGE)"</span>
                                    <div class="kpi-value">{move || format!("{:.1}", sleep.get().iter().map(|s| s.sleep_seconds).sum::<f64>() / 3600.0)}<span class="unit">"H"</span></div>
                                </div>
                            </div>
                            <p class="muted" style="margin-top:12px">"Anapavsis = 300 + sleep recharge − activity drain − stress. Stress is an HR-elevation strain score (no HRV on the watch). Sleep long, train hard, and the battery stays near the 300."</p>
                        </section>
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
                            <div class="panel-head"><h2>"AI PROVIDER (LOCAL OR REMOTE)"</h2></div>
                            <div class="settings-grid">
                                <label class="ctl">"BASE URL (CHAT COMPLETIONS)"
                                    <input prop:value=ai_base on:input=move |ev| set_ai_base.set(event_target_value(&ev)) placeholder="http://localhost:11434/v1" />
                                </label>
                                <label class="ctl">"MODEL"
                                    <input prop:value=ai_model on:input=move |ev| set_ai_model.set(event_target_value(&ev)) placeholder="llama3" />
                                </label>
                                <label class="ctl">"API KEY"
                                    <input prop:value=ai_key on:input=move |ev| set_ai_key.set(event_target_value(&ev)) placeholder="(blank for local)" />
                                </label>
                            </div>
                            <p class="muted" style="margin-top:12px">"One OpenAI-compatible protocol for local (Ollama, LM Studio, llama.cpp) and remote providers — only the URL differs, so everything can stay on your machine."</p>
                            <button class="btn" style="margin-top:12px" on:click=move |_| persist_settings()>"SAVE"</button>
                        </section>
                    }.into_view(),
                    Tab::Skopos => view! {
                        <TabHero tab=Tab::Skopos />
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
            </footer>
        </div>
    }
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
    // runs once per selection).
    let (stats, set_stats) = create_signal(None::<SessionStats>);
    let (stats_error, set_stats_error) = create_signal(None::<String>);
    let (loading, set_loading) = create_signal(true);
    let sid_fetch = id.clone();
    spawn_local(async move {
        match fetch_session_stats(&base, &token, &sid_fetch).await {
            Ok(s) => set_stats.set(Some(s)),
            Err(e) => set_stats_error.set(Some(e)),
        }
        set_loading.set(false);
    });

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
                        </div>
                    }
                }}
            </Show>
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
