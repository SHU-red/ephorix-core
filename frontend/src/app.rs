//! Root application: header, timeline, sessions, types. Black/red Spartan UI.
//! UI preferences (series visibility, range) are persisted in the backend
//! DB (`user_settings`) — the web app needs no second volume.

use leptos::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::api::*;
use crate::icons::{glyph_key, glyph_svg, GLYPH_KEYS, LAMBDA};
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
#[component]
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
    let (loading, set_loading) = create_signal(false);
    let (selected_type, set_selected_type) = create_signal(None::<String>);
    let (bucket_info, set_bucket_info) = create_signal(String::new());
    let (series, set_series) = create_signal(SeriesConfig::default());
    let (settings_loaded, set_settings_loaded) = create_signal(false);
    let clear_sel = create_rw_signal(0u32);
    let reset_zoom = create_rw_signal(0u32);
    let (editing_type, set_editing_type) = create_signal(None::<String>);
    let (show_settings, set_show_settings) = create_signal(false);
    let (nutrition, set_nutrition) = create_signal(Vec::<NutritionEvent>::new());
    let (sleep, set_sleep) = create_signal(Vec::<SleepDay>::new());
    let (detections, set_detections) = create_signal(Vec::<Detection>::new());
    let (zoom_mode, set_zoom_mode) = create_signal(true);
    let (ai_text, set_ai_text) = create_signal(String::new());
    let (ai_base, set_ai_base) = create_signal(String::new());
    let (ai_model, set_ai_model) = create_signal(String::new());
    let (ai_key, set_ai_key) = create_signal(String::new());
    let (body_score, set_body_score) = create_signal(None::<f64>);

    // Create-form state.
    let (new_type_name, set_new_type_name) = create_signal(String::new());
    let (new_type_color, set_new_type_color) = create_signal("#E53935".to_string());
    let (new_type_icon, set_new_type_icon) = create_signal(GLYPH_KEYS[0].to_string());

    // Persist series visibility + range to the backend settings.
    let persist_settings = move || {
        let base = base.get();
        let token = token.get();
        let s = series.get();
        let d = days.get();
        let ai = json!({ "baseUrl": ai_base.get(), "model": ai_model.get(), "apiKey": ai_key.get() });
        spawn_local(async move {
            let body = json!({
                "series": s,
                "rangeDays": d,
                "aiProvider": ai,
            });
            let _ = put_settings(&base, &token, &body).await;
        });
    };

    let refresh = move || {
        set_loading.set(true);
        set_error.set(None);
        let base = base.get();
        let token = token.get();
        let days = days.get();
        let loaded = settings_loaded.get();
        let to_ms = js_sys::Date::now();
        let from_ms = to_ms - days as f64 * 86_400_000.0;
        let bucket = nice_bucket((to_ms - from_ms) / 1000.0 / 800.0);

        spawn_local(async move {
            match fetch_timeline(&base, &token, from_ms, to_ms, &bucket).await {
                Ok(tt) => {
                    set_bucket_info.set(format!("bucket: {}", tt.bucket));
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
            match fetch_body_score(&base, &token, from_ms, to_ms).await {
                Ok(s) => set_body_score.set(s),
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
                    set_settings_loaded.set(true);
                }
            }
            set_loading.set(false);
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

    let toggle_series = move |key: &'static str| {
        let mut s = series.get();
        match key {
            "heartRate" => s.heart_rate = !s.heart_rate,
            "steps" => s.steps = !s.steps,
            "calories" => s.calories = !s.calories,
            _ => {}
        }
        set_series.set(s);
        persist_settings();
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

    // -- Agoge types CRUD ----------------------------------------------------

    let add_type = move |_| {
        let name = new_type_name.get().trim().to_string();
        if name.is_empty() {
            return;
        }
        let base = base.get();
        let token = token.get();
        let color = new_type_color.get();
        let icon = new_type_icon.get();
        spawn_local(async move {
            let body = json!({ "name": name, "colorCode": color, "icon": icon });
            match post_json(&base, &token, "/api/v1/agoge-types", &body).await {
                Ok(_) => {
                    set_new_type_name.set(String::new());
                    refresh();
                }
                Err(e) => set_error.set(Some(e)),
            }
        });
    };

    let update_type = move |id: String, name: String, color: String, icon: String| {
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            let body = json!({ "name": name, "colorCode": color, "icon": icon });
            match put_json(&base, &token, &format!("/api/v1/agoge-types/{id}"), &body).await {
                Ok(_) => {
                    set_editing_type.set(None);
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
                Ok(_) => refresh(),
                Err(e) => set_error.set(Some(e)),
            }
        });
    };

    let accept_det = move |id: String| {
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            match accept_detection(&base, &token, &id).await {
                Ok(_) => refresh(),
                Err(e) => set_error.set(Some(e)),
            }
        });
    };

    let reject_det = move |id: String| {
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            match reject_detection(&base, &token, &id).await {
                Ok(_) => refresh(),
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
                    refresh();
                }
                Err(e) => set_error.set(Some(e)),
            }
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
                        <h1>"EPHORIX"</h1>
                        <img class="brand-mark" src="/assets/helmet_transparent.png" alt="Spartan helmet" />
                    </div>
                    <span class="sub">"ΑΓΩΓΗ · TRAINING COMMAND"</span>
                </div>
                <div class="controls">
                    <button class="btn" on:click=move |_| refresh() prop:disabled=loading>
                        {move || if loading.get() { "LOADING…" } else { "SYNC" }}
                    </button>
                    <button class="btn" class:on=move || show_settings.get() on:click=move |_| set_show_settings.update(|v| *v = !*v)>
                        "SETTINGS"
                    </button>
                </div>
            </header>
            <div class="meander-rule"></div>

            <Show when=move || show_settings.get() fallback=|| ()>
                <section class="panel settings-panel">
                    <div class="panel-head"><h2>"SETTINGS"</h2></div>
                    <div class="settings-grid">
                        <label class="ctl">
                            "API BASE URL"
                            <input
                                prop:value=base
                                on:input=move |ev| set_base.set(event_target_value(&ev))
                                spellcheck="false"
                                placeholder="BLANK = SAME-ORIGIN"
                            />
                        </label>
                        <label class="ctl">
                            "TOKEN"
                            <input
                                prop:value=token
                                on:input=move |ev| set_token.set(event_target_value(&ev))
                                spellcheck="false"
                            />
                        </label>
                    </div>
                    <div class="panel-head" style="margin-top:18px"><h2>"AI PROVIDER (LOCAL OR REMOTE)"</h2></div>
                    <div class="settings-grid">
                        <label class="ctl">
                            "BASE URL (CHAT COMPLETIONS)"
                            <input
                                prop:value=ai_base
                                on:input=move |ev| set_ai_base.set(event_target_value(&ev))
                                spellcheck="false"
                                placeholder="http://localhost:11434/v1"
                            />
                        </label>
                        <label class="ctl">
                            "MODEL"
                            <input
                                prop:value=ai_model
                                on:input=move |ev| set_ai_model.set(event_target_value(&ev))
                                spellcheck="false"
                                placeholder="llama3"
                            />
                        </label>
                        <label class="ctl">
                            "API KEY"
                            <input
                                prop:value=ai_key
                                on:input=move |ev| set_ai_key.set(event_target_value(&ev))
                                spellcheck="false"
                                placeholder="(blank for local)"
                            />
                        </label>
                    </div>
                    <div class="settings-grid" style="margin-top:12px">
                        <label class="ctl">
                            "DESCRIBE A MEAL / DRINK"
                            <input
                                prop:value=ai_text
                                on:input=move |ev| set_ai_text.set(event_target_value(&ev))
                                spellcheck="false"
                                placeholder="two eggs and a slice of toast"
                            />
                        </label>
                        <button class="btn" on:click=ai_submit>"AI LOG"</button>
                        <button class="btn" on:click=move |_| persist_settings()>"SAVE"</button>
                    </div>
                    <p class="muted" style="margin-top:10px">
                        "AI LOG sends the description to the provider, which returns kcal (food) or ml (water); it is logged to your nutrition. Local providers (Ollama, LM Studio, llama.cpp) and remote OpenAI-compatible endpoints use the same URL+model."
                    </p>
                </section>
            </Show>

            <Show when=move || error.get().is_some() fallback=|| ()>
                <div class="banner-error">{move || error.get().unwrap_or_default()}</div>
            </Show>

            <main>
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
                            {move || body_score.get().map(|s| format!("{s:.0}")).unwrap_or_else(|| "—".to_string())}
                            <span class="unit">"/100"</span>
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
                        </div>
                    </div>
                    <div class="stats-line">
                        {move || {
                            let pts = points.get();
                            let kcal = pts.iter().filter_map(|p| p.active_calories).sum::<f64>();
                            if pts.is_empty() {
                                "no data".to_string()
                            } else {
                                format!("{} buckets · {:.0} kcal · {}", pts.len(), kcal, bucket_info.get())
                            }
                        }}
                    </div>
                    <div class="series-toggles">
                        <span class="muted">"METRICS"</span>
                        <button class="pill" class:on=move || series.get().heart_rate class:off=move || !series.get().heart_rate on:click=move |_| toggle_series("heartRate")>
                            <span class="dot dot-hr"></span>"HR"
                        </button>
                        <button class="pill" class:on=move || series.get().steps class:off=move || !series.get().steps on:click=move |_| toggle_series("steps")>
                            <span class="dot dot-steps"></span>"STEPS"
                        </button>
                        <button class="pill" class:on=move || series.get().calories class:off=move || !series.get().calories on:click=move |_| toggle_series("calories")>
                            <span class="dot dot-kcal"></span>"KCAL"
                        </button>
                        <span class="muted cursor-readout">
                            "CURSOR "
                            {move || cursor.get().map(fmt_time).unwrap_or_else(|| "—".to_string())}
                        </span>
                    </div>
                    <TimelineChart
                        points=points
                        sessions=sessions
                        types=types
                        nutrition=nutrition
                        sleep=sleep
                        series=series
                        selection=set_selection
                        cursor=set_cursor
                        zoom_mode=zoom_mode
                        clear_trigger=clear_sel
                        reset_zoom=reset_zoom
                    />
                    <div class="timeline-actions">
                        <button class="btn" class:on=move || !zoom_mode.get() on:click=toggle_zoom_mode>
                            {move || if zoom_mode.get() { "DRAG = ZOOM" } else { "DRAG = SELECT" }}
                        </button>
                        <button class="btn" on:click=do_reset_zoom>
                            "RESET ZOOM"
                        </button>
                        <button class="btn" on:click=create_from_selection>
                            "CREATE SESSION FROM SELECTION"
                        </button>
                        <button class="btn" on:click=clear_selection>
                            "CLEAR SELECTION"
                        </button>
                        <button class="btn" on:click=close_open_at_cursor>
                            "CLOSE OPEN AT CURSOR"
                        </button>
                        <label class="ctl">
                            "TYPE"
                            <select on:change=move |ev| set_selected_type.set(option_value(&ev))>
                                <option value="">"UNDEFINED"</option>
                                <For
                                    each=move || types.get()
                                    key=|t| t.id.clone()
                                    let:t
                                >
                                    <option value=t.id.clone()>{t.name.clone()}</option>
                                </For>
                            </select>
                        </label>
                        <span class="muted selection-readout">
                            "SELECTION: "
                            {move || {
                                selection.get().map(|(f, t)| {
                                    format!("{} → {}", fmt_time(f.min(t)), fmt_time(f.max(t)))
                                }).unwrap_or_else(|| "—".to_string())
                            }}
                        </span>
                    </div>
                </section>

                <section class="panel">
                    <div class="panel-head">
                        <h2>"DETECTED ACTIVITIES"</h2>
                        <span class="muted">
                            {move || {
                                let p = detections.get().iter().filter(|d| d.status == "proposed").count();
                                format!("{p} proposed · accept or reject")
                            }}
                        </span>
                    </div>
                    <Show
                        when=move || detections.get().iter().any(|d| d.status == "proposed")
                        fallback=|| view! { <p class="muted">"No proposed activities in this range. Train with the watch or ingest data and they will surface here."</p> }
                    >
                        <ul class="list detections">
                            <For
                                each=move || detections.get().into_iter().filter(|d| d.status == "proposed")
                                key=|d| d.id.clone()
                                let:d
                            >
                                {move || {
                                    let label = format!(
                                        "{} · {} – {} · peak {} bpm",
                                        d.proposed_type_name.clone().unwrap_or_else(|| "Activity".to_string()),
                                        fmt_time(d.start),
                                        fmt_time(d.end),
                                        d.peak_hr as i64
                                    );
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

                <div class="lower">
                    <section class="panel">
                        <div class="panel-head">
                            <h2>"AGOGE SESSIONS"</h2>
                            <span class="muted">{move || format!("{} total", sessions.get().len())}</span>
                        </div>
                        <ul class="list">
                            <For
                                each=move || { let _ = types.get(); sessions.get() }
                                key=|s| s.id.clone()
                                let:s
                            >
                                <SessionRow
                                    session=s
                                    types=types
                                    on_delete=Callback::new(move |id: String| delete_session(id))
                                />
                            </For>
                        </ul>
                    </section>

                    <section class="panel">
                        <div class="panel-head">
                            <h2>"AGOGE TYPES"</h2>
                            <span class="muted">{move || format!("{} types", types.get().len())}</span>
                        </div>
                        <ul class="list">
                            <For
                                each=move || types.get()
                                key=|t| t.id.clone()
                                let:t
                            >
                                {move || {
                                    let t_edit = t.clone();
                                    let t_edit_id = t_edit.id.clone();
                                    let t_view = t.clone();
                                    if editing_type.get().as_ref() == Some(&t.id) {
                                        view! {
                                            <TypeEditRow
                                                ty=t_edit
                                                on_save=Callback::new(move |(n, c, i): (String, String, String)| update_type(t_edit_id.clone(), n, c, i))
                                                on_cancel=Callback::new(move |_| set_editing_type.set(None))
                                            />
                                        }.into_view()
                                    } else {
                                        view! {
                                            <TypeRow
                                                ty=t_view
                                                on_edit=Callback::new(move |id: String| set_editing_type.set(Some(id)))
                                                on_delete=Callback::new(move |id: String| delete_type(id))
                                            />
                                        }.into_view()
                                    }
                                }}
                            </For>
                        </ul>
                        <div class="type-create">
                            <input
                                prop:value=new_type_name
                                on:input=move |ev| set_new_type_name.set(event_target_value(&ev))
                                placeholder="NEW TYPE NAME"
                                maxlength="40"
                            />
                            <input type="color" prop:value=new_type_color
                                on:input=move |ev| set_new_type_color.set(event_target_value(&ev)) />
                            <GlyphPicker value=new_type_icon set=set_new_type_icon />
                            <button class="btn" on:click=add_type>"ADD"</button>
                        </div>
                    </section>
                </div>
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

/// One Agoge Type row (glyph + name + color + edit/delete).
#[component]
fn TypeRow(
    ty: AgogeType,
    on_edit: Callback<String>,
    on_delete: Callback<String>,
) -> impl IntoView {
    let id = ty.id.clone();
    let id_edit = id.clone();
    let id_del = id.clone();
    let name = ty.name.clone();
    let glyph = glyph_svg(glyph_key(&ty.icon));
    let color = ty.color_code.clone();
    view! {
        <li class="row">
            <span class="dot" style=format!("background:{color}")></span>
            <span class="row-icon" inner_html=glyph></span>
            <span class="row-name">{name}</span>
            <button class="btn small" on:click=move |_| on_edit.call(id_edit.clone())>
                "EDIT"
            </button>
            <button class="btn small" on:click=move |_| on_delete.call(id_del.clone())>
                "DELETE"
            </button>
        </li>
    }
}

/// Inline edit form for one Agoge Type.
#[component]
fn TypeEditRow(
    ty: AgogeType,
    on_save: Callback<(String, String, String)>,
    on_cancel: Callback<()>,
) -> impl IntoView {
    let (name, set_name) = create_signal(ty.name.clone());
    let (color, set_color) = create_signal(ty.color_code.clone());
    let (icon, set_icon) = create_signal(ty.icon.clone());
    view! {
        <li class="row edit">
            <input
                prop:value=name
                on:input=move |ev| set_name.set(event_target_value(&ev))
                maxlength="40"
            />
            <input type="color" prop:value=color
                on:input=move |ev| set_color.set(event_target_value(&ev)) />
            <GlyphPicker value=icon set=set_icon />
            <button class="btn small"
                on:click=move |_| on_save.call((name.get(), color.get(), icon.get()))>
                "SAVE"
            </button>
            <button class="btn small" on:click=move |_| on_cancel.call(())>
                "CANCEL"
            </button>
        </li>
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
