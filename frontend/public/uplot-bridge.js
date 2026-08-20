/* EphoriX uPlot bridge.
 *
 * Loaded after uPlot.min.js (both from static/). Exposes window.EphoriX,
 * which the Leptos wasm code calls via #[wasm_bindgen(js_namespace = EphoriX)].
 *
 * Responsibilities:
 *  - chart lifecycle (create / setData / destroy)
 *  - linked x-axis groups: two charts share one x-domain, kept in lock-step
 *    across create, setData, zoom and reset
 *  - direction-aware drag zoom: horizontal drag -> full-height band (x-only),
 *    vertical drag -> full-width band (y-only), diagonal -> box (both)
 *  - zoom-level-aware time axis labels (hours / days / weeks / months / years)
 *  - cursor time + coordinate mapping for the session overlay
 *  - x-scale change listeners (DOM overlays that must track the zoom)
 */
(function (global) {
  "use strict";

  var charts = new Map();
  var nextId = 1;

  var MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  var DAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
  var DAY_MS = 86400000;

  function pad(n) {
    return (n < 10 ? "0" : "") + n;
  }

  /* X-axis label, driven by the tick INCREMENT (not the dataset span), so the
     unit scales with the zoom level:
       < 1 day    -> "14:30"
       < 7 days   -> "Mon 5"        (days)
       < 28 days  -> "5 Aug"        (weeks)
       < 366 days -> "Aug" / "Aug '26" (months; year shown on January)
       >= 1 year  -> "2026"         (years)
     Resolves the "__ephorix_time__" sentinel injected from Rust opts JSON. */
  function fmtTick(ms, incrMs) {
    var d = new Date(ms);
    if (incrMs < DAY_MS) {
      return pad(d.getHours()) + ":" + pad(d.getMinutes());
    }
    if (incrMs < 7 * DAY_MS) {
      return DAYS[d.getDay()] + " " + d.getDate();
    }
    if (incrMs < 28 * DAY_MS) {
      return d.getDate() + " " + MONTHS[d.getMonth()];
    }
    if (incrMs < 366 * DAY_MS) {
      var m = MONTHS[d.getMonth()];
      return d.getMonth() === 0 ? m + " '" + String(d.getFullYear()).slice(2) : m;
    }
    return String(d.getFullYear());
  }

  function timeAxisValues(u, splits) {
    if (!splits || splits.length === 0) return [];
    var xmin = u.scales.x.min;
    var xmax = u.scales.x.max;
    if (xmin == null || xmax == null) {
      var xs = u.data && u.data[0];
      if (!xs || !xs.length) return [];
      xmin = xs[0];
      xmax = xs[xs.length - 1];
    }
    var spanMs = xmax - xmin;
    var incrMs = splits.length >= 2 ? splits[1] - splits[0] : spanMs;
    return splits.map(function (t) { return fmtTick(t, incrMs); });
  }

  /* Per-chart hover tooltip: a div positioned inside the uPlot root that
     follows the cursor and lists each series value colored by its stroke.
     Appended to `setCursor` (never assigned) so it coexists with `onCursor`. */
  function setupTooltip(c, meta) {
    if (!meta || !meta.length) return;
    var root = c.root;
    if (getComputedStyle(root).position === "static") {
      root.style.position = "relative";
    }
    var tip = document.createElement("div");
    tip.className = "ephorix-tooltip";
    tip.style.display = "none";
    root.appendChild(tip);
    if (!c.hooks.setCursor) c.hooks.setCursor = [];
    c.hooks.setCursor.push(function (u) {
      var idx = u.cursor.idx;
      if (idx === null || idx === undefined || idx < 0) {
        tip.style.display = "none";
        return;
      }
      var d = new Date(u.data[0][idx]);
      var html = '<span class="ephorix-tip-time">' + pad(d.getHours()) + ":" + pad(d.getMinutes()) + "</span>";
      for (var i = 0; i < meta.length; i++) {
        var m = meta[i];
        var v = u.data[m.seriesIdx][idx];
        if (v === null || v === undefined) continue;
        html += '<span class="ephorix-tip-sep"> · </span><span style="color:' + m.color + '">' + m.label + " " + Math.round(v) + "</span>";
      }
      tip.innerHTML = html;
      tip.style.display = "block";
      tip.style.left = (u.bbox.left + u.cursor.left + 14) + "px";
      tip.style.top = (u.bbox.top + u.cursor.top - 12) + "px";
    });
  }

  function create(elId, optsJson, dataJson) {
    var el = document.getElementById(elId);
    if (!el) throw new Error("ephorix-bridge: no element #" + elId);

    var opts = JSON.parse(optsJson);
    var data = JSON.parse(dataJson);

    // Bars and hover points are requested declaratively from Rust; resolve
    // to uPlot paths / point functions here. Capture tooltip labels + colors
    // before uPlot normalizes the series config.
    var seriesMeta = [];
    (opts.series || []).forEach(function (s, i) {
      if (s && s.bars) {
        s.paths = uPlot.paths.bars({ size: [0, 30], align: 0 });
        delete s.bars;
      }
      if (s && s.points && s.points.show === "hover") {
        s.points.show = function (u, seriesIdx) {
          var idx = u.cursor.idx;
          return idx === null || idx === undefined || idx < 0 ? null : [idx];
        };
        s.points.filter = function (u, seriesIdx, show) { return show; };
      }
      if (s && i > 0 && s.tooltip) {
        seriesMeta.push({ seriesIdx: i, label: s.tooltip, color: s.stroke || "#fff" });
      }
    });

    // Zoom-aware time labels (sentinel from Rust opts JSON).
    if (opts.axes && opts.axes[0] && opts.axes[0].values === "__ephorix_time__") {
      opts.axes[0].values = timeAxisValues;
    }

    var chart = new uPlot(opts, data, el);
    var id = nextId++;
    charts.set(id, chart);
    setupTooltip(chart, seriesMeta);
    return id;
  }

  function setData(id, dataJson) {
    var c = charts.get(id);
    if (!c) return;
    c.setData(JSON.parse(dataJson));
    var group = groupOf(id);
    if (group) {
      recomputeFull(group);
      if (group.full) setGroupX(group, group.full[0], group.full[1]);
    }
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
    links.delete(id);
    interactions.delete(id);
    scaleCbs.delete(id);
  }

  /* ------------------------------------------------------------------------
   * Linked x-axis groups.
   *
   * A group holds the ids of charts that must share an x-domain, plus the
   * full (unzoomed) domain. Every mutation that affects x — create, setData,
   * zoom, reset — writes the same explicit range to every member, so the two
   * diagrams can never drift (uPlot's per-chart auto-range is the only thing
   * that would otherwise let them diverge).
   * ---------------------------------------------------------------------- */
  var links = new Map();   // chart id -> group id
  var groups = new Map();  // group id -> { ids: Set, full: [min,max] | null }
  var nextGroupId = 1;
  var scaleCbs = new Map(); // chart id -> Set of x-scale-change callbacks

  function groupOf(id) {
    var gid = links.get(id);
    return gid == null ? null : groups.get(gid);
  }

  /* x-scale change listeners: cb() fires (no args) after the chart's x-scale
     changes via any path below (zoom, reset, setData resync, link). */
  function onScaleChange(id, cb) {
    var set = scaleCbs.get(id);
    if (!set) { set = new Set(); scaleCbs.set(id, set); }
    set.add(cb);
  }

  function fireScaleChange(id) {
    // uPlot applies setScale on the next animation frame; defer so callbacks
    // read the post-zoom scale (overlay/strip re-position correctly).
    requestAnimationFrame(function () {
      var set = scaleCbs.get(id);
      if (!set || set.size === 0) return;
      set.forEach(function (cb) { cb(); });
    });
  }

  function setGroupX(group, lo, hi) {
    if (lo == null || hi == null) return;
    if (hi - lo < 1) { lo -= 60000; hi += 60000; } // single-point guard
    group.ids.forEach(function (id) {
      var c = charts.get(id);
      if (c) c.setScale("x", { min: lo, max: hi });
      fireScaleChange(id);
    });
  }

  function recomputeFull(group) {
    var min = Infinity;
    var max = -Infinity;
    group.ids.forEach(function (id) {
      var c = charts.get(id);
      var xs = c && c.data && c.data[0];
      if (xs && xs.length) {
        min = Math.min(min, xs[0]);
        max = Math.max(max, xs[xs.length - 1]);
      }
    });
    group.full = min === Infinity ? null : [min, max];
    return group.full;
  }

  function linkX(idA, idB) {
    var a = charts.get(idA);
    var b = charts.get(idB);
    if (!a || !b) return;

    var gid = links.get(idA) != null ? links.get(idA)
      : (links.get(idB) != null ? links.get(idB) : nextGroupId++);
    var group = groups.get(gid) || { ids: new Set(), full: null };

    // Merge the two groups if the charts were previously in different ones.
    [idA, idB].forEach(function (id) {
      var old = links.get(id);
      if (old != null && old !== gid) {
        var other = groups.get(old);
        if (other) {
          other.ids.forEach(function (oid) { group.ids.add(oid); links.set(oid, gid); });
          groups.delete(old);
        }
      }
      links.set(id, gid);
      group.ids.add(id);
    });

    groups.set(gid, group);
    var full = recomputeFull(group);
    if (full) setGroupX(group, full[0], full[1]);
  }

  /* ------------------------------------------------------------------------
   * Zoom.
   * ---------------------------------------------------------------------- */
  var zoomModes = new Map();

  function setZoomMode(id, isZoom) {
    zoomModes.set(id, !!isZoom);
  }

  function zoomTo(id, x0, x1, y0, y1, dir) {
    var c = charts.get(id);
    if (!c) return;
    if (dir !== "y") {
      var group = groupOf(id);
      if (group) {
        setGroupX(group, Math.min(x0, x1), Math.max(x0, x1));
      } else {
        c.setScale("x", { min: Math.min(x0, x1), max: Math.max(x0, x1) });
        fireScaleChange(id);
      }
    }
    if (dir !== "x") {
      c.setScale("y", { min: Math.min(y0, y1), max: Math.max(y0, y1) });
    }
    c.setSelect({ left: 0, top: 0, width: 0, height: 0 }, true);
  }

  function resetZoom(id) {
    var c = charts.get(id);
    if (!c) return;
    var group = groupOf(id);
    if (group) {
      recomputeFull(group);
      if (group.full) {
        setGroupX(group, group.full[0], group.full[1]);
      } else {
        group.ids.forEach(function (gid) {
          var cc = charts.get(gid);
          if (cc) cc.setScale("x", { min: null, max: null });
          fireScaleChange(gid);
        });
      }
    } else {
      c.setScale("x", { min: null, max: null });
      fireScaleChange(id);
    }
    c.setScale("y", { min: null, max: null });
    if (c.scales.y2) c.setScale("y2", { min: null, max: null });
    if (c.scales.y3) c.setScale("y3", { min: null, max: null });
  }

  /* Direction-aware drag with snapping visual feedback:
       horizontal -> full-height band (x-only zoom)
       vertical   -> full-width band (y-only zoom)
       diagonal   -> box (both axes)
     In select mode (zoom off) the drag is always a full-height x band.
     Pointer capture keeps the gesture alive when released outside the chart.

     `onDrag` and `onClick` share one set of pointerdown/move/up listeners per
     chart: a clean left-click (total movement < 5px) fires the click callback
     with the epoch-ms timestamp under the cursor, while a drag fires the drag
     callback — never both. */
  var interactions = new Map(); // chart id -> shared pointer state

  function ensureInteraction(id) {
    var c = charts.get(id);
    if (!c) return null;
    if (interactions.has(id)) return interactions.get(id);

    var root = c.root;

    // uPlot's root (.uplot) is not positioned; make it so the overlay's
    // absolute coordinates are measured against the same origin as c.bbox.
    if (getComputedStyle(root).position === "static") {
      root.style.position = "relative";
    }

    var state = {
      dragCb: null,
      clickCb: null,
      overlay: null,
      dragging: false,
      startX: 0,
      startY: 0
    };
    interactions.set(id, state);

    function ensureOverlay() {
      if (state.overlay) return;
      state.overlay = document.createElement("div");
      state.overlay.className = "ephorix-zoom-rect";
      state.overlay.style.cssText =
        "position:absolute;pointer-events:none;border:1px solid #ff5252;" +
        "background:rgba(229,57,53,0.16);z-index:20;display:none;";
      root.appendChild(state.overlay);
    }

    function dirOf(dx, dy) {
      var ratio = Math.abs(dx) / Math.max(1, Math.abs(dy));
      return ratio > 2 ? "x" : (ratio < 0.5 ? "y" : "both");
    }

    function clamp(v, lo, hi) {
      return Math.max(lo, Math.min(v, hi));
    }

    /* Root-coordinate rect with the snap applied, clamped to the plot area. */
    function compute(cx, cy) {
      var b = c.bbox;
      var dx = cx - state.startX;
      var dy = cy - state.startY;
      var dir = dirOf(dx, dy);
      var zoom = zoomModes.get(id) !== false;
      if (!zoom) dir = "x"; // select mode: always an x-range band

      var l, t, w, h;
      if (dir === "x") {
        l = Math.min(state.startX, cx); w = Math.abs(dx); t = b.top; h = b.height;
      } else if (dir === "y") {
        l = b.left; w = b.width; t = Math.min(state.startY, cy); h = Math.abs(dy);
      } else {
        l = Math.min(state.startX, cx); w = Math.abs(dx);
        t = Math.min(state.startY, cy); h = Math.abs(dy);
      }

      var L = clamp(l, b.left, b.left + b.width);
      var T = clamp(t, b.top, b.top + b.height);
      var W = clamp(w, 0, b.left + b.width - L);
      var H = clamp(h, 0, b.top + b.height - T);
      return { dir: dir, l: L, t: T, w: W, h: H };
    }

    function draw(cx, cy) {
      var r = compute(cx, cy);
      state.overlay.style.left = r.l + "px";
      state.overlay.style.top = r.t + "px";
      state.overlay.style.width = Math.max(2, r.w) + "px";
      state.overlay.style.height = Math.max(2, r.h) + "px";
      state.overlay.style.display = "block";
    }

    root.addEventListener("pointerdown", function (e) {
      if (e.button !== 0) return;
      ensureOverlay();
      var r = root.getBoundingClientRect();
      state.startX = e.clientX - r.left;
      state.startY = e.clientY - r.top;
      state.dragging = true;
      if (root.setPointerCapture) {
        try { root.setPointerCapture(e.pointerId); } catch (err) { /* no-op */ }
      }
      e.preventDefault();
    });

    root.addEventListener("pointermove", function (e) {
      if (!state.dragging) return;
      var r = root.getBoundingClientRect();
      var cx = e.clientX - r.left;
      var cy = e.clientY - r.top;
      if (Math.abs(cx - state.startX) < 3 && Math.abs(cy - state.startY) < 3) return;
      draw(cx, cy);
    });

    function finish(e) {
      if (!state.dragging) return;
      state.dragging = false;
      var r = root.getBoundingClientRect();
      var cx = e.clientX - r.left;
      var cy = e.clientY - r.top;
      if (state.overlay) state.overlay.style.display = "none";

      // Clean click: route to onClick (if any) and never the drag callback.
      if (Math.abs(cx - state.startX) < 5 && Math.abs(cy - state.startY) < 5) {
        if (state.clickCb) {
          state.clickCb(c.posToVal(cx - c.bbox.left, "x"));
        }
        return;
      }

      var rec = compute(cx, cy);
      var b = c.bbox;
      if (state.dragCb) {
        state.dragCb(JSON.stringify({
          x0: c.posToVal(rec.l - b.left, "x"),
          x1: c.posToVal(rec.l - b.left + rec.w, "x"),
          y0: c.posToVal(rec.t - b.top, "y"),
          y1: c.posToVal(rec.t - b.top + rec.h, "y"),
          dir: rec.dir
        }));
      }
    }

    root.addEventListener("pointerup", finish);
    root.addEventListener("pointercancel", function () {
      state.dragging = false;
      if (state.overlay) state.overlay.style.display = "none";
    });

    return state;
  }

  function onDrag(id, cb) {
    var s = ensureInteraction(id);
    if (s) s.dragCb = cb;
  }

  function onClick(id, cb) {
    var s = ensureInteraction(id);
    if (s) s.clickCb = cb;
  }

  function onCursor(id, cb) {
    var c = charts.get(id);
    if (!c) return;
    if (!c.hooks.setCursor) c.hooks.setCursor = [];
    c.hooks.setCursor.push(function (u) {
      var idx = u.cursor.idx;
      cb(idx === null || idx === undefined ? null : u.data[0][idx]);
    });
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
    onClick: onClick,
    onScaleChange: onScaleChange,
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
