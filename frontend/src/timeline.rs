//! uPlot timeline component: raw health metrics + Agoge session overlay bars.
//!
//! Downsampling strategy: the client asks the server for a bucket size that
//! yields <= ~800 points over the visible range (`nice_bucket`); the server
//! aggregates with TimescaleDB `time_bucket`. The chart then renders raw
//! buckets without further decimation.
//!
//! Series (heart rate / steps / active kcal) can be shown or hidden via
//! `SeriesConfig`; visibility is persisted in the backend settings.

use leptos::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use wasm_bindgen::{closure::Closure, prelude::wasm_bindgen, JsCast};

use crate::api::{fmt_time, ms_from_iso, AgogeSession, AgogeType, NutritionEvent, SleepDay, TimelinePoint};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesConfig {
    #[serde(default = "default_true")]
    pub heart_rate: bool,
    #[serde(default = "default_true")]
    pub steps: bool,
    #[serde(default = "default_true")]
    pub calories: bool,
}

impl Default for SeriesConfig {
    fn default() -> Self {
        Self {
            heart_rate: true,
            steps: true,
            calories: true,
        }
    }
}

fn default_true() -> bool {
    true
}

#[wasm_bindgen]
extern "C" {
    // Bridge arguments are JSON strings: the wasm->JS boundary (via
    // serde_wasm_bindgen) proved lossy, JSON.parse/stringify does not.
    #[wasm_bindgen(js_namespace = EphoriX, js_name = create)]
    fn ephorix_create(el_id: &str, opts_json: &str, data_json: &str) -> u32;
    #[wasm_bindgen(js_namespace = EphoriX, js_name = setData)]
    fn ephorix_set_data(id: u32, data_json: &str);
    #[wasm_bindgen(js_namespace = EphoriX, js_name = setSeriesShow)]
    fn ephorix_set_series_show(id: u32, series_idx: u32, show: bool);
    #[wasm_bindgen(js_namespace = EphoriX, js_name = onDrag)]
    fn ephorix_on_drag(id: u32, cb: &js_sys::Function);
    #[wasm_bindgen(js_namespace = EphoriX, js_name = zoomTo)]
    fn ephorix_zoom_to(id: u32, x0: f64, x1: f64, y0: f64, y1: f64, dir: &str);
    #[wasm_bindgen(js_namespace = EphoriX, js_name = onCursor)]
    fn ephorix_on_cursor(id: u32, cb: &js_sys::Function);
    #[wasm_bindgen(js_namespace = EphoriX, js_name = valToPos)]
    fn ephorix_val_to_pos(id: u32, val: f64) -> f64;
    #[wasm_bindgen(js_namespace = EphoriX, js_name = plotBBox)]
    fn ephorix_plot_bbox(id: u32) -> String;
    #[wasm_bindgen(js_namespace = EphoriX, js_name = clearSelection)]
    fn ephorix_clear_selection(id: u32);
    #[wasm_bindgen(js_namespace = EphoriX, js_name = resetZoom)]
    fn ephorix_reset_zoom(id: u32);
}

#[derive(Debug, Deserialize)]
struct BBox {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

#[derive(Debug, Deserialize)]
struct DragRect {
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
    dir: String,
}

#[component]
pub fn TimelineChart(
    points: ReadSignal<Vec<TimelinePoint>>,
    sessions: ReadSignal<Vec<AgogeSession>>,
    types: ReadSignal<Vec<AgogeType>>,
    nutrition: ReadSignal<Vec<NutritionEvent>>,
    sleep: ReadSignal<Vec<SleepDay>>,
    series: ReadSignal<SeriesConfig>,
    selection: WriteSignal<Option<(f64, f64)>>,
    cursor: WriteSignal<Option<f64>>,
    zoom_mode: ReadSignal<bool>,
    clear_trigger: RwSignal<u32>,
    reset_zoom: RwSignal<u32>,
) -> impl IntoView {
    let chart_ref: NodeRef<leptos::html::Div> = create_node_ref();
    let overlay_ref: NodeRef<leptos::html::Div> = create_node_ref();
    let chart_id = create_rw_signal(0u32);

    // Clear the drag-selection rectangle on demand (bumped by the parent).
    create_effect(move |_| {
        let _ = clear_trigger.get();
        let id = chart_id.get();
        if id != 0 {
            ephorix_clear_selection(id);
        }
    });

    // Reset all axes on demand (bumped by the RESET ZOOM button / range change).
    create_effect(move |_| {
        let _ = reset_zoom.get();
        let id = chart_id.get();
        if id != 0 {
            ephorix_reset_zoom(id);
        }
    });
    // One-time chart creation once the div is mounted. `get_untracked` keeps
    // data updates from re-creating the chart.
    create_effect(move |_| {
        if let Some(el) = chart_ref.get() {
            let width = el.client_width().max(320) as i32;
            let opts = build_opts(width);
            let data = build_data(points.get_untracked());
            let id = ephorix_create(
                "ephorix-chart",
                &opts.to_string(),
                &data.to_string(),
            );
            chart_id.set(id);

            let drag_cb = Closure::wrap(Box::new(move |json: String| {
                let Ok(r) = serde_json::from_str::<DragRect>(&json) else { return };
                if zoom_mode.get_untracked() {
                    ephorix_zoom_to(id, r.x0, r.x1, r.y0, r.y1, &r.dir);
                } else {
                    selection.set(Some((r.x0, r.x1)));
                }
            }) as Box<dyn FnMut(String)>);
            ephorix_on_drag(id, drag_cb.as_ref().unchecked_ref());
            drag_cb.forget();

            let cur_cb = Closure::wrap(Box::new(move |ts: Option<f64>| {
                cursor.set(ts);
            }) as Box<dyn FnMut(Option<f64>)>);
            ephorix_on_cursor(id, cur_cb.as_ref().unchecked_ref());
            cur_cb.forget();
        }
    });

    // Push new data into the existing chart.
    create_effect(move |_| {
        let id = chart_id.get();
        if id == 0 {
            return;
        }
        let data = build_data(points.get());
        ephorix_set_data(id, &data.to_string());
    });

    // Apply series visibility toggles.
    create_effect(move |_| {
        let id = chart_id.get();
        if id == 0 {
            return;
        }
        let s = series.get();
        ephorix_set_series_show(id, 1, s.heart_rate);
        ephorix_set_series_show(id, 2, s.steps);
        ephorix_set_series_show(id, 3, s.calories);
    });

    // Re-render the overlay (session bars + sleep bands + nutrition markers)
    // when any of its inputs change.
    create_effect(move |_| {
        let id = chart_id.get();
        if id == 0 {
            return;
        }
        let sess = sessions.get();
        let types = types.get();
        let nut = nutrition.get();
        let slp = sleep.get();
        let _ = points.get(); // re-render once raw data is charted
        if let Some(overlay) = overlay_ref.get() {
            render_overlay(&overlay, id, &sess, &types, &nut, &slp);
        }
    });

    view! {
        <div class="chart-wrap">
            <div node_ref=chart_ref class="chart" id="ephorix-chart"></div>
            <div node_ref=overlay_ref class="session-overlay"></div>
            <Show when=move || points.get().is_empty() fallback=|| ()>
                <div class="chart-empty">
                    <span class="chart-empty-mark" inner_html=crate::icons::LAMBDA></span>
                    <p>"NO DATA"</p>
                </div>
            </Show>
        </div>
    }
}

fn build_opts(width: i32) -> serde_json::Value {
    json!({
        "width": width,
        "height": 380,
        "legend": { "show": true, "live": true },
        "cursor": { "show": true, "points": { "show": true } },
        "select": { "show": true, "color": "rgba(229, 57, 53, 0.28)" },
        "scales": {
            "x": { "time": true },
            "y": { "range": [0, null] },
            "y2": { "range": [0, null] },
            "y3": { "range": [0, null] }
        },
        "axes": [
            { "side": 2, "size": 32, "stroke": "#3a3a3a", "grid": { "stroke": "#161616" },
              "ticks": { "size": 80 }, "font": "10px 'IBM Plex Mono', monospace",
              "values": "__ephorix_time__" },
            { "side": 3, "size": 46, "stroke": "#e53935", "grid": { "stroke": "#141414" },
              "ticks": { "size": 60 }, "font": "10px 'IBM Plex Mono', monospace" },
            { "side": 3, "size": 38, "scale": "y2", "stroke": "#4a4a4a", "grid": { "show": false },
              "ticks": { "size": 70 }, "font": "10px 'IBM Plex Mono', monospace" },
            { "side": 1, "size": 38, "scale": "y3", "stroke": "#7a3a3a", "grid": { "show": false },
              "ticks": { "size": 70 }, "font": "10px 'IBM Plex Mono', monospace" }
        ],
        "series": [
            {},
            { "label": "Heart rate (bpm)", "stroke": "#e53935", "width": 2.5,
              "fill": "rgba(229, 57, 53, 0.10)", "points": { "show": false } },
            { "label": "Steps", "stroke": "#8f8f8f", "fill": "rgba(143, 143, 143, 0.14)",
              "scale": "y2", "bars": true, "points": { "show": false } },
            { "label": "Active kcal", "stroke": "#ff5252", "width": 1.5, "dash": [6, 4],
              "scale": "y3", "points": { "show": false } }
        ]
    })
}

/// uPlot data rows: [x epoch-ms, heart_rate, steps, active_kcal].
fn build_data(points: Vec<TimelinePoint>) -> serde_json::Value {
    let xs: Vec<f64> = points.iter().map(|p| p.ts).collect();
    let hr: Vec<Option<f64>> = points.iter().map(|p| p.heart_rate).collect();
    let steps: Vec<Option<f64>> = points.iter().map(|p| p.steps.map(|s| s as f64)).collect();
    let kcal: Vec<Option<f64>> = points.iter().map(|p| p.active_calories).collect();
    json!([xs, hr, steps, kcal])
}

fn render_overlay(
    container: &leptos::html::HtmlElement<leptos::html::Div>,
    chart_id: u32,
    sessions: &[AgogeSession],
    types: &[AgogeType],
    nutrition: &[NutritionEvent],
    sleep: &[SleepDay],
) {
    let doc = web_sys::window().unwrap().document().unwrap();
    container.set_attribute("style", "").ok();
    container.set_inner_html("");

    let bbox: BBox = serde_json::from_str(&ephorix_plot_bbox(chart_id))
        .unwrap_or(BBox { left: 0.0, top: 0.0, width: 0.0, height: 0.0 });
    if bbox.width <= 0.0 {
        return;
    }
    let _ = container.set_attribute(
        "style",
        &format!(
            "position:absolute;left:{}px;top:{}px;width:{}px;height:{}px;",
            bbox.left, bbox.top, bbox.width, bbox.height
        ),
    );

    let now = js_sys::Date::now();
    for s in sessions {
        let Some(from) = ms_from_iso(&s.start_time) else { continue };
        let to = s.end_time.as_deref().and_then(ms_from_iso).unwrap_or(now);
        if to < from {
            continue;
        }
        let x = ephorix_val_to_pos(chart_id, from);
        let w = (ephorix_val_to_pos(chart_id, to) - x).abs().max(2.0);

        let matched = s.type_id.as_ref().and_then(|tid| types.iter().find(|t| &t.id == tid));
        let color = matched.map(|t| t.color_code.clone()).unwrap_or_else(|| "#7B0000".to_string());
        let name = matched.map(|t| t.name.clone()).unwrap_or_else(|| "Undefined".to_string());

        let el = doc
            .create_element("div")
            .unwrap()
            .dyn_into::<web_sys::HtmlDivElement>()
            .unwrap();
        el.set_class_name(if s.status == "active" { "session-bar open" } else { "session-bar" });
        let _ = el.set_attribute("title", &format!("{name} · {} – {}", fmt_time(from), fmt_time(to)));
        let _ = el.set_attribute(
            "style",
            &format!("position:absolute;top:0;bottom:0;left:{x}px;width:{w}px;background:{color};"),
        );
        let _ = container.append_child(&el);
    }

    // Sleep bands: a thin strip at the bottom, one per day. Sleep is a daily
    // sum on the watch, so the band is anchored at an assumed 07:00 wake and
    // spans the reported sleep duration backwards (approximate).
    const WAKE_HOUR_MS: f64 = 7.0 * 3_600_000.0;
    for s in sleep {
        if s.sleep_seconds <= 0.0 {
            continue;
        }
        let wake = s.ts + WAKE_HOUR_MS;
        let from = wake - s.sleep_seconds * 1000.0;
        let x = ephorix_val_to_pos(chart_id, from);
        let w = (ephorix_val_to_pos(chart_id, wake) - x).abs().max(2.0);
        let el = doc.create_element("div").unwrap().dyn_into::<web_sys::HtmlDivElement>().unwrap();
        el.set_class_name("sleep-band");
        let hrs = s.sleep_seconds / 3600.0;
        let _ = el.set_attribute("title", &format!("sleep {:.1}h (restful {:.1}h)", hrs, s.restful_seconds / 3600.0));
        let _ = el.set_attribute(
            "style",
            &format!("position:absolute;left:{x}px;width:{w}px;bottom:0;height:14px;"),
        );
        let _ = container.append_child(&el);
    }

    // Nutrition markers: small dots at the top edge (red = food, gray = water).
    for n in nutrition {
        let x = ephorix_val_to_pos(chart_id, n.ts);
        let color = if n.kind == "water" { "#8f8f8f" } else { "#ff5252" };
        let label = if n.kind == "water" {
            format!("water {:.0} ml", n.amount)
        } else {
            format!("food {:.0} kcal", n.amount)
        };
        let title = match &n.note {
            Some(note) if !note.is_empty() => format!("{label} · {note}"),
            _ => label,
        };
        let el = doc.create_element("div").unwrap().dyn_into::<web_sys::HtmlDivElement>().unwrap();
        el.set_class_name("nutrition-dot");
        let _ = el.set_attribute("title", &title);
        let _ = el.set_attribute(
            "style",
            &format!("position:absolute;left:{x}px;top:0;width:7px;height:7px;background:{color};"),
        );
        let _ = container.append_child(&el);
    }
}
