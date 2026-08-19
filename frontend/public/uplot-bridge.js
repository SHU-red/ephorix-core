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

  var zoomModes = new Map();

  function setZoomMode(id, isZoom) {
    zoomModes.set(id, !!isZoom);
  }

  /* Custom drag with direction-aware visual feedback. The rectangle snaps to
     the full opposite axis for single-axis zooms — a full-height band for a
     horizontal (x) zoom, a full-width band for a vertical (y) zoom, and the
     raw rectangle for area zoom — so the user sees exactly what section they
     are grabbing. Reports data coords + direction on release. */
  function onDrag(id, cb) {
    var c = charts.get(id);
    if (!c) return;

    var root = c.root;
    var overlay = null;
    var dragging = false;
    var startX = 0, startY = 0;

    function ensureOverlay() {
      if (overlay) return;
      overlay = document.createElement("div");
      overlay.style.position = "absolute";
      overlay.style.pointerEvents = "none";
      overlay.style.border = "1px solid #ff5252";
      overlay.style.background = "rgba(229, 57, 53, 0.16)";
      overlay.style.zIndex = "20";
      overlay.style.display = "none";
      root.appendChild(overlay);
    }

    function dirOf(dx, dy) {
      var ratio = Math.abs(dx) / Math.max(1, Math.abs(dy));
      return ratio > 2 ? "x" : (ratio < 0.5 ? "y" : "both");
    }

    function clamp(v, lo, hi) {
      return Math.max(lo, Math.min(v, hi));
    }

    function draw(cx, cy) {
      var dx = cx - startX, dy = cy - startY;
      var dir = dirOf(dx, dy);
      var b = c.bbox;
      var l, t, w, h;
      if (dir === "x") {
        l = Math.min(startX, cx); w = Math.abs(dx); t = b.top; h = b.height;
      } else if (dir === "y") {
        l = b.left; w = b.width; t = Math.min(startY, cy); h = Math.abs(dy);
      } else {
        l = Math.min(startX, cx); w = Math.abs(dx); t = Math.min(startY, cy); h = Math.abs(dy);
      }
      var snap = zoomModes.get(id) !== false;
      if (!snap) {
        l = Math.min(startX, cx); w = Math.abs(dx); t = Math.min(startY, cy); h = Math.abs(dy);
      }
      overlay.style.left = clamp(l, b.left, b.left + b.width) + "px";
      overlay.style.top = clamp(t, b.top, b.top + b.height) + "px";
      overlay.style.width = Math.max(2, w) + "px";
      overlay.style.height = Math.max(2, h) + "px";
      overlay.style.display = "block";
    }

    root.addEventListener("pointerdown", function (e) {
      if (e.button !== 0) return;
      ensureOverlay();
      var r = root.getBoundingClientRect();
      startX = e.clientX - r.left;
      startY = e.clientY - r.top;
      dragging = true;
      e.preventDefault();
    });

    root.addEventListener("pointermove", function (e) {
      if (!dragging) return;
      var r = root.getBoundingClientRect();
      var cx = e.clientX - r.left, cy = e.clientY - r.top;
      if (Math.abs(cx - startX) < 3 && Math.abs(cy - startY) < 3) return;
      draw(cx, cy);
    });

    root.addEventListener("pointerup", function (e) {
      if (!dragging) return;
      dragging = false;
      if (overlay) overlay.style.display = "none";
      var r = root.getBoundingClientRect();
      var cx = e.clientX - r.left, cy = e.clientY - r.top;
      var dx = cx - startX, dy = cy - startY;
      if (Math.abs(dx) < 5 && Math.abs(dy) < 5) return; // click, not drag

      var dir = dirOf(dx, dy);
      var b = c.bbox;
      var xlo = clamp(Math.min(startX, cx), b.left, b.left + b.width);
      var xhi = clamp(Math.max(startX, cx), b.left, b.left + b.width);
      var ylo = clamp(Math.min(startY, cy), b.top, b.top + b.height);
      var yhi = clamp(Math.max(startY, cy), b.top, b.top + b.height);

      var snap = zoomModes.get(id) !== false;
      if (snap) {
        if (dir === "x") { ylo = b.top; yhi = b.top + b.height; }
        else if (dir === "y") { xlo = b.left; xhi = b.left + b.width; }
      }

      cb(JSON.stringify({
        x0: c.posToVal(xlo - b.left, "x"),
        x1: c.posToVal(xhi - b.left, "x"),
        y0: c.posToVal(ylo - b.top, "y"),
        y1: c.posToVal(yhi - b.top, "y"),
        dir: snap ? dir : "x"
      }));
    });
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

  /* Lock the x-axis of two charts together (drag-zoom on one follows the
     other). Guarded against feedback loops. */
  function linkX(idA, idB) {
    var a = charts.get(idA), b = charts.get(idB);
    if (!a || !b) return;
    var syncing = false;
    function sync(from, to) {
      if (syncing || !to) return;
      syncing = true;
      try {
        to.setScale("x", { min: from.scales.x.min, max: from.scales.x.max });
      } finally {
        syncing = false;
      }
    }
    a.hooks.setScale = [function (u, key) { if (key === "x") sync(u, b); }];
    b.hooks.setScale = [function (u, key) { if (key === "x") sync(u, a); }];
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
    setZoomMode: setZoomMode,
    linkX: linkX,
    resetZoom: resetZoom,
    onCursor: onCursor,
    getSelection: getSelection,
    clearSelection: clearSelection,
    valToPos: valToPos,
    plotBBox: plotBBox,
    _charts: charts
  };
})(window);
