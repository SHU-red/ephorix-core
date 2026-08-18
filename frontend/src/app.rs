//! Root application: header, timeline, sessions, types. Black/red Spartan UI.

use leptos::*;
use serde_json::json;

use crate::api::*;
use crate::timeline::TimelineChart;

#[component]
pub fn App() -> impl IntoView {
    let (token, set_token) = create_signal("ephorix-dev-1".to_string());
    let (base, set_base) = create_signal("http://localhost:3000".to_string());
    let (days, set_days) = create_signal(7i64);
    let (types, set_types) = create_signal(Vec::<AgogeType>::new());
    let (sessions, set_sessions) = create_signal(Vec::<AgogeSession>::new());
    let (points, set_points) = create_signal(Vec::<TimelinePoint>::new());
    let (selection, set_selection) = create_signal(None::<(f64, f64)>);
    let (cursor, set_cursor) = create_signal(None::<f64>);
    let (error, set_error) = create_signal(None::<String>);
    let (loading, set_loading) = create_signal(false);
    let (selected_type, set_selected_type) = create_signal(None::<String>);
    let (new_type_name, set_new_type_name) = create_signal(String::new());
    let (bucket_info, set_bucket_info) = create_signal(String::new());

    let refresh = move || {
        set_loading.set(true);
        set_error.set(None);
        let base = base.get();
        let token = token.get();
        let days = days.get();
        let to_ms = js_sys::Date::now();
        let from_ms = to_ms - days as f64 * 86_400_000.0;
        let bucket = nice_bucket((to_ms - from_ms) / 1000.0 / 800.0);

        spawn_local(async move {
            match fetch_timeline(&base, &token, from_ms, to_ms, &bucket).await {
                Ok(tt) => {
                    set_bucket_info.set(format!("bucket: {}", tt.bucket));
                    set_points.set(tt.points);
                    set_sessions.set(tt.sessions);
                }
                Err(e) => set_error.set(Some(e)),
            }
            match fetch_types(&base, &token).await {
                Ok(t) => set_types.set(t),
                Err(e) => set_error.set(Some(e)),
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
                    set_selection.set(None);
                    refresh();
                }
                Err(e) => set_error.set(Some(e)),
            }
        });
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

    let add_type = move |_| {
        let name = new_type_name.get().trim().to_string();
        if name.is_empty() {
            return;
        }
        let base = base.get();
        let token = token.get();
        spawn_local(async move {
            match post_json(&base, &token, "/api/v1/agoge-types", &json!({ "name": name })).await {
                Ok(_) => {
                    set_new_type_name.set(String::new());
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

    // -----------------------------------------------------------------------

    view! {
        <div class="app">
            <header class="header">
                <div class="brand">
                    <h1>"EPHORIX"</h1>
                    <span class="sub">"ΑΓΩΓΗ · TRAINING COMMAND"</span>
                </div>
                <div class="controls">
                    <label class="ctl">
                        "BASE"
                        <input
                            prop:value=base
                            on:input=move |ev| set_base.set(event_target_value(&ev))
                            spellcheck="false"
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
                    <label class="ctl">
                        "RANGE"
                        <select
                            on:change=move |ev| set_days.set(event_target_value(&ev).parse().unwrap_or(7))
                        >
                            <option value="1">"1 DAY"</option>
                            <option value="3">"3 DAYS"</option>
                            <option value="7" selected>"7 DAYS"</option>
                            <option value="30">"30 DAYS"</option>
                        </select>
                    </label>
                    <button class="btn" on:click=move |_| refresh() prop:disabled=loading>
                        {move || if loading.get() { "LOADING…" } else { "SYNC" }}
                    </button>
                </div>
            </header>

            <Show when=move || error.get().is_some() fallback=|| ()>
                <div class="banner-error">{move || error.get().unwrap_or_default()}</div>
            </Show>

            <main>
                <section class="panel">
                    <div class="panel-head">
                        <h2>"RAW METRICS / AG OGE OVERLAY"</h2>
                        <span class="muted">
                            {move || {
                                let pts = points.get();
                                let kcal = pts.iter().filter_map(|p| p.active_calories).sum::<f64>();
                                if pts.is_empty() {
                                    "no data".to_string()
                                } else {
                                    format!("{} buckets · {:.0} kcal · {}", pts.len(), kcal, bucket_info.get())
                                }
                            }}
                        </span>
                    </div>
                    <TimelineChart
                        points=points
                        sessions=sessions
                        types=types
                        selection=set_selection
                        cursor=set_cursor
                    />
                    <div class="timeline-actions">
                        <button class="btn" on:click=create_from_selection>
                            "CREATE SESSION FROM SELECTION"
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
                        <span class="muted cursor-readout">
                            "CURSOR: "
                            {move || cursor.get().map(fmt_time).unwrap_or_else(|| "—".to_string())}
                        </span>
                    </div>
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
                                <TypeRow
                                    ty=t
                                    on_delete=Callback::new(move |id: String| delete_type(id))
                                />
                            </For>
                        </ul>
                        <div class="inline-form">
                            <input
                                prop:value=new_type_name
                                on:input=move |ev| set_new_type_name.set(event_target_value(&ev))
                                placeholder="NEW TYPE NAME"
                            />
                            <button class="btn" on:click=add_type>"ADD"</button>
                        </div>
                    </section>
                </div>
            </main>

            <footer>
                "EPHORIX · RAW METRICS ARE SACRED · SESSIONS ARE DISCIPLINE"
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

/// One Agoge Type row.
#[component]
fn TypeRow(ty: AgogeType, on_delete: Callback<String>) -> impl IntoView {
    let id = ty.id.clone();
    let name = ty.name.clone();
    let icon = ty.icon.clone();
    let color = ty.color_code.clone();
    view! {
        <li class="row">
            <span class="dot" style=format!("background:{color}")></span>
            <span class="row-name">{name}</span>
            <span class="row-time muted">{icon}</span>
            <button class="btn small" on:click=move |_| on_delete.call(id.clone())>
                "DELETE"
            </button>
        </li>
    }
}
