const API = {
  health: "/health",
  stats: "/api/v1/stats",
  domains: "/api/v1/domains/custom",
};

const state = {
  domains: [],
  filter: "",
  pollerId: null,
};

const $ = (id) => document.getElementById(id);

const el = {
  health: $("api-health"),
  refreshBtn: $("refresh-btn"),
  addForm: $("add-domain-form"),
  domainInput: $("domain-input"),
  domainSearch: $("domain-search"),
  domainCount: $("domain-count"),
  domainList: $("domain-list"),
  toastStack: $("toast-stack"),
  lastUpdated: $("last-updated"),

  // summary bar
  statQueries: $("stat-queries"),
  statBlocked: $("stat-blocked"),
  statBlockPct: $("stat-block-pct"),
  statCacheRate: $("stat-cache-rate"),
  statDomainsCount: $("stat-domains-count"),

  // metrics detail
  mTotal: $("m-total"),
  mBlocked: $("m-blocked"),
  mCacheHits: $("m-cache-hits"),
  mCacheMisses: $("m-cache-misses"),
  mLatency: $("m-latency"),
  mErrors: $("m-errors"),
  mUptime: $("m-uptime"),
};

// ── Formatters ──

function fmtInt(n) {
  return new Intl.NumberFormat().format(Number(n) || 0);
}

function fmtPct(n) {
  return (Number(n) || 0).toFixed(1) + "%";
}

function fmtMs(n) {
  const v = Number(n) || 0;
  return v < 1 ? "<1 ms" : v.toFixed(1) + " ms";
}

function fmtUptime(sec) {
  sec = Math.max(0, Number(sec) || 0);
  const d = Math.floor(sec / 86400);
  const h = Math.floor((sec % 86400) / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = sec % 60;
  if (d > 0) return d + "d " + h + "h " + m + "m";
  if (h > 0) return h + "h " + m + "m " + s + "s";
  return m + "m " + s + "s";
}

function fmtAgo(date) {
  const diff = Math.floor((Date.now() - date.getTime()) / 1000);
  if (diff < 5) return "just now";
  if (diff < 60) return diff + "s ago";
  return Math.floor(diff / 60) + "m ago";
}

function normalizeDomain(v) {
  return v
    .trim()
    .toLowerCase()
    .replace(/^\.+|\.+$/g, "");
}

function isValidDomain(v) {
  if (!v || v.length > 253 || v.includes(" ")) return false;
  const labels = v.split(".");
  if (labels.length < 2) return false;
  return labels.every((l) => /^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$/.test(l));
}

// ── API ──

async function api(url, opts = {}) {
  const res = await fetch(url, {
    headers: {
      Accept: "application/json",
      ...(opts.body ? { "Content-Type": "application/json" } : {}),
    },
    ...opts,
  });
  const text = await res.text();
  const data = text ? JSON.parse(text) : {};
  if (!res.ok)
    throw new Error(data.message || "Request failed (" + res.status + ")");
  return data;
}

// ── Toast ──

function toast(msg, type) {
  const node = document.createElement("div");
  node.className = "toast toast-" + type;
  node.textContent = msg;
  el.toastStack.appendChild(node);
  setTimeout(() => node.remove(), 3200);
}

// ── Health ──

function setHealth(kind, text) {
  el.health.className = "status status-" + kind;
  el.health.querySelector(".status-dot");
  el.health.lastChild.textContent = text;
}

async function loadHealth() {
  try {
    const res = await fetch(API.health);
    setHealth(res.ok ? "ok" : "warning", res.ok ? "Healthy" : "Degraded");
  } catch {
    setHealth("error", "Unreachable");
  }
}

// ── Stats ──

let lastStatsTime = null;

function renderStats(s) {
  // Summary bar
  el.statQueries.textContent = fmtInt(s.total_queries);
  el.statBlocked.textContent = fmtInt(s.blocked_queries);
  el.statBlockPct.textContent =
    s.total_queries > 0 ? fmtPct(s.block_percentage) : "";
  el.statCacheRate.textContent = fmtPct(s.cache_hit_percentage);
  el.statDomainsCount.textContent = fmtInt(s.blocked_domains_count);

  // Detail table
  el.mTotal.textContent = fmtInt(s.total_queries);
  el.mBlocked.textContent = fmtInt(s.blocked_queries);
  el.mCacheHits.textContent = fmtInt(s.cache_hits);
  el.mCacheMisses.textContent = fmtInt(s.cache_misses);
  el.mLatency.textContent = fmtMs(s.average_upstream_latency_ms);
  el.mErrors.textContent = fmtInt(s.upstream_errors);
  el.mUptime.textContent = fmtUptime(s.uptime_seconds);

  // Highlight errors only if non-zero
  el.mErrors.classList.toggle("val-error", s.upstream_errors > 0);

  lastStatsTime = new Date();
  el.lastUpdated.textContent = "Last updated: just now";
}

async function loadStats() {
  const s = await api(API.stats);
  renderStats(s);
}

// ── Domains ──

function renderDomains() {
  const q = state.filter.toLowerCase();
  const filtered = state.domains.filter((d) => d.includes(q));

  el.domainCount.textContent =
    filtered.length === state.domains.length
      ? state.domains.length
      : filtered.length + " / " + state.domains.length;

  el.domainList.replaceChildren();

  if (filtered.length === 0) {
    const li = document.createElement("li");
    li.className = "empty-msg";
    li.textContent = q ? "No matches." : "No custom domains yet.";
    el.domainList.appendChild(li);
    return;
  }

  for (const domain of filtered) {
    const li = document.createElement("li");

    const name = document.createElement("span");
    name.className = "domain-name";
    name.textContent = domain;

    const btn = document.createElement("button");
    btn.className = "btn-remove";
    btn.type = "button";
    btn.textContent = "×";
    btn.dataset.domain = domain;
    btn.title = "Unblock " + domain;

    li.appendChild(name);
    li.appendChild(btn);
    el.domainList.appendChild(li);
  }
}

async function loadDomains() {
  const data = await api(API.domains);
  state.domains = Array.isArray(data.domains)
    ? data.domains.slice().sort()
    : [];
  renderDomains();
}

async function addDomain(raw) {
  const d = normalizeDomain(raw);
  if (!isValidDomain(d))
    throw new Error("Invalid domain. Use format: example.com");
  return api(API.domains, {
    method: "POST",
    body: JSON.stringify({ domain: d }),
  });
}

async function removeDomain(d) {
  return api(API.domains, {
    method: "DELETE",
    body: JSON.stringify({ domain: normalizeDomain(d) }),
  });
}

// ── Handlers ──

async function refreshAll() {
  el.refreshBtn.disabled = true;
  try {
    await Promise.all([loadHealth(), loadStats(), loadDomains()]);
  } finally {
    el.refreshBtn.disabled = false;
  }
}

async function onAddSubmit(e) {
  e.preventDefault();
  const val = el.domainInput.value;
  try {
    const res = await addDomain(val);
    toast(res.message || "Blocked.", "success");
    el.domainInput.value = "";
    await loadDomains();
  } catch (err) {
    toast(err.message, "error");
  }
}

async function onListClick(e) {
  const btn = e.target.closest("[data-domain]");
  if (!btn) return;
  const domain = btn.dataset.domain;
  if (!confirm("Unblock " + domain + "?")) return;
  btn.disabled = true;
  try {
    const res = await removeDomain(domain);
    toast(
      res.message || "Unblocked.",
      res.status === "success" ? "success" : "error",
    );
    await loadDomains();
  } catch (err) {
    toast(err.message, "error");
  }
}

// ── Update "last updated" label ──

function tickLastUpdated() {
  if (!lastStatsTime) return;
  el.lastUpdated.textContent = "Last updated: " + fmtAgo(lastStatsTime);
}

// ── Boot ──

function init() {
  el.refreshBtn.addEventListener("click", () =>
    refreshAll().catch((e) => toast(e.message, "error")),
  );
  el.addForm.addEventListener("submit", onAddSubmit);
  el.domainSearch.addEventListener("input", (e) => {
    state.filter = e.target.value || "";
    renderDomains();
  });
  el.domainList.addEventListener("click", onListClick);

  window.addEventListener("visibilitychange", () => {
    if (!document.hidden) loadStats().catch(() => {});
  });

  refreshAll().catch((e) => toast(e.message, "error"));

  // Poll stats every 5 seconds
  state.pollerId = setInterval(() => {
    loadStats().catch(() => setHealth("warning", "Delayed"));
  }, 5000);

  // Update relative timestamp every 5 seconds
  setInterval(tickLastUpdated, 5000);
}
init();
