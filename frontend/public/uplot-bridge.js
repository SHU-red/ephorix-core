/* EphoriX uPlot bridge.
 *
 * Loaded after uPlot.min.js (both from static/). Exposes window.EphoriX,
 * which the Leptos wasm code calls via #[wasm_bindgen(js_namespace = EphoriX)].
 *
 * Responsibilities:
 *  - chart lifecycle (create / setData / destroy)
 *  - drag-to-select range reporting (create session over selection)
 *  - cursor time reporting (close open session at cursor)
 *  - coordinate mapping for the session overlay bars (valToPos / plotBBox)
 */
(function (global) {
  "use strict";

  var charts = new Map();
  var nextId = 1;

  var MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

  function pad(n) {
    return (n < 10 ? "0" : "") + n;
  }

  /* Compact, span-aware x-axis labels (resolves the "__ephorix_time__"
     sentinel that Rust injects into the opts JSON). */
  function fmtTime(ms, spanSec) {
    var d = new Date(ms);
    if (spanSec <= 36 * 3600) {
      return pad(d.getHours()) + ":" + pad(d.getMinutes());
    }
    if (spanSec <= 45 * 86400) {
      return MONTHS[d.getMonth()] + " " + d.getDate();
    }
    return MONTHS[d.getMonth()] + " '" + String(d.getFullYear()).slice(2);
  }

  function timeAxisValues(u, splits) {
    var data = u.data;
    var xs = data[0];
    var spanSec = (xs[xs.length - 1] - xs[0]) / 1000;
    return splits.map(function (t) { return fmtTime(t, spanSec); });
  }

  function create(elId, optsJson, dataJson) {
    var el = document.getElementById(elId);
    if (!el) throw new Error("ephorix-bridge: no element #" + elId);

    var opts = JSON.parse(optsJson);
    var data = JSON.parse(dataJson);

    // Bars are requested declaratively from Rust; resolve to uPlot paths here.
    (opts.series || []).forEach(function (s) {
      if (s && s.bars) {
        s.paths = uPlot.paths.bars({ size: [0, 60], align: 0 });
        delete s.bars;
      }
    });

    // Compact span-aware time labels (sentinel from Rust opts JSON).
    if (opts.axes && opts.axes[0] && opts.axes[0].values === "__ephorix_time__") {
      opts.axes[0].values = timeAxisValues;
    }

    var chart = new uPlot(opts, data, el);
    var id = nextId++;
    charts.set(id, chart);
    return id;
  }

  function setData(id, dataJson) {
    var c = charts.get(id);
    if (c) c.setData(JSON.parse(dataJson));
  }

  /* Show/hide one series (1-based: 1=HR, 2=Steps, 3=kcal). */
  function setSeriesShow(id, seriesIdx, show) {
    var c = charts.get(id);
    if (c) c.setSeries(seriesIdx, { show: !!show });
  }

  function destroy(id) {
    var c = charts.get(id);
    if (c) {
      c.destroy();
      charts.delete(id);
    }
  }

  /* Direction-aware drag: reports the rectangle in data coords plus a
     direction hint. Wider-than-tall => "x", taller-than-wide => "y",
     roughly square => "both". The Rust side decides zoom vs select. */
  function onDrag(id, cb) {
    var c = charts.get(id);
    if (!c) return;
    c.hooks.setSelect = [function (u) {
      var sel = u.select;
      if (!sel || sel.width <= 0 || sel.height <= 0) return;
      var ratio = sel.width / Math.max(1, sel.height);
      var dir = ratio > 2 ? "x" : (ratio < 0.5 ? "y" : "both");
      cb(JSON.stringify({
        x0: u.posToVal(sel.left, "x"),
        x1: u.posToVal(sel.left + sel.width, "x"),
        y0: u.posToVal(sel.top, "y"),
        y1: u.posToVal(sel.top + sel.height, "y"),
        dir: dir
      }));
    }];
  }

  function zoomTo(id, x0, x1, y0, y1, dir) {
    var c = charts.get(id);
    if (!c) return;
    if (dir !== "y") {
      c.setScale("x", { min: Math.min(x0, x1), max: Math.max(x0, x1) });
    }
    if (dir !== "x") {
      c.setScale("y", { min: Math.min(y0, y1), max: Math.max(y0, y1) });
    }
    c.setSelect({ left: 0, top: 0, width: 0, height: 0 }, true);
  }

  function resetZoom(id) {
    var c = charts.get(id);
    if (!c) return;
    c.setScale("x", { min: null, max: null });
    c.setScale("y", { min: null, max: null });
    c.setScale("y2", { min: null, max: null });
    c.setScale("y3", { min: null, max: null });
  }
  function onCursor(id, cb) {
    var c = charts.get(id);
    if (!c) return;
    c.hooks.setCursor = [function (u) {
      var idx = u.cursor.idx;
      cb(idx === null || idx === undefined ? null : u.data[0][idx]);
    }];
  }

  function getSelection(id) {
    var c = charts.get(id);
    if (!c) return null;
    var sel = c.select;
    if (!sel || sel.width <= 0) return null;
    return {
      from: c.posToVal(sel.left, "x"),
      to: c.posToVal(sel.left + sel.width, "x")
    };
  }

  function clearSelection(id) {
    var c = charts.get(id);
    if (!c) return;
    c.setSelect({ left: 0, top: 0, width: 0, height: 0 }, true);
  }

  /* Plot-area-relative px for a timestamp (x scale). Inner-rect space,
   * matching plotBBox, so overlay bars line up with the plotted data. */
  function valToPos(id, val) {
    var c = charts.get(id);
    return c ? c.valToPos(val, "x") : 0;
  }

  /* Plot-area bbox in px, used to position the session overlay exactly over
   * the axes' inner area. Returned as JSON string for wasm consumption. */
  function plotBBox(id) {
    var c = charts.get(id);
    if (!c) return "{}";
    var b = c.bbox;
    if (!b) return "{}";
    return JSON.stringify({
      left: typeof b.left === "number" ? b.left : 0,
      top: typeof b.top === "number" ? b.top : 0,
      width: typeof b.width === "number" ? b.width : 0,
      height: typeof b.height === "number" ? b.height : 0
    });
  }

  global.EphoriX = {
    create: create,
    setData: setData,
    setSeriesShow: setSeriesShow,
    destroy: destroy,
    onDrag: onDrag,
    zoomTo: zoomTo,
    resetZoom: resetZoom,
    onCursor: onCursor,
    getSelection: getSelection,
    clearSelection: clearSelection,
    valToPos: valToPos,
    plotBBox: plotBBox,
    _charts: charts
  };
})(window);
