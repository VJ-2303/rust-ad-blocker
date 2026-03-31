const API = {
  health: "/health",
  stats: "/api/v1/stats",
  domains: "/api/v1/domains/custom",
};

const state = {
  domains: [],
  domainFilter: "",
  pollerId: null,
};

const refs = {
  refreshButton: document.getElementById("refresh-all"),
  healthBadge: document.getElementById("api-health"),
  statsUpdatedAt: document.getElementById("stats-updated-at"),
  addDomainForm: document.getElementById("add-domain-form"),
  domainInput: document.getElementById("domain-input"),
  domainSearch: document.getElementById("domain-search"),
  domainCount: document.getElementById("domain-count"),
  domainTableBody: document.getElementById("domain-table-body"),
  toastStack: document.getElementById("toast-stack"),
  metrics: {
    totalQueries: document.getElementById("metric-total-queries"),
    blockedQueries: document.getElementById("metric-blocked-queries"),
    blockRate: document.getElementById("metric-block-rate"),
    cacheHits: document.getElementById("metric-cache-hits"),
    cacheMisses: document.getElementById("metric-cache-misses"),
    cacheHitRate: document.getElementById("metric-cache-hit-rate"),
    latency: document.getElementById("metric-latency"),
    uptime: document.getElementById("metric-uptime"),
    errors: document.getElementById("metric-errors"),
  },
};

function formatInt(value) {
  const parsed = Number(value) || 0;
  return new Intl.NumberFormat().format(parsed);
}

function formatPercentage(value) {
  const parsed = Number(value) || 0;
  return `${parsed.toFixed(2)}%`;
}

function formatLatency(value) {
  const parsed = Number(value) || 0;
  return `${parsed.toFixed(2)} ms`;
}

function formatUptime(totalSeconds) {
  const sec = Math.max(0, Number(totalSeconds) || 0);
  const days = Math.floor(sec / 86400);
  const hours = Math.floor((sec % 86400) / 3600);
  const minutes = Math.floor((sec % 3600) / 60);
  const seconds = sec % 60;

  if (days > 0) {
    return `${days}d ${hours}h ${minutes}m`;
  }

  if (hours > 0) {
    return `${hours}h ${minutes}m ${seconds}s`;
  }

  return `${minutes}m ${seconds}s`;
}

function normalizeDomain(value) {
  return value.trim().toLowerCase().replace(/^\.+|\.+$/g, "");
}

function isLikelyDomain(value) {
  if (!value || value.length > 253 || value.includes(" ")) {
    return false;
  }

  const labels = value.split(".");
  if (labels.length < 2) {
    return false;
  }

  return labels.every((label) => /^[a-z0-9-]{1,63}$/.test(label) && !label.startsWith("-") && !label.endsWith("-"));
}

async function readResponseJson(response) {
  const text = await response.text();

  if (!text) {
    return {};
  }

  try {
    return JSON.parse(text);
  } catch (error) {
    throw new Error("Server returned invalid JSON.");
  }
}

async function apiRequest(url, options = {}) {
  const response = await fetch(url, {
    headers: {
      Accept: "application/json",
      ...(options.body ? { "Content-Type": "application/json" } : {}),
      ...(options.headers || {}),
    },
    ...options,
  });

  const payload = await readResponseJson(response).catch(() => ({}));

  if (!response.ok) {
    const message = payload.message || `Request failed (${response.status}).`;
    throw new Error(message);
  }

  return payload;
}

function setHealthStatus(kind, text) {
  refs.healthBadge.textContent = text;
  refs.healthBadge.className = `health-pill health-pill-${kind}`;
}

function showToast(message, type = "success") {
  const node = document.createElement("div");
  node.className = `toast ${type}`;
  node.textContent = message;
  refs.toastStack.appendChild(node);

  window.setTimeout(() => {
    node.remove();
  }, 3400);
}

function setLoadingDomains(message = "Loading domains...") {
  refs.domainTableBody.innerHTML = `<tr><td colspan="2" class="placeholder">${message}</td></tr>`;
}

function updateDomainCountLabel(count, filteredCount) {
  if (count === filteredCount) {
    refs.domainCount.textContent = `${count} domain${count === 1 ? "" : "s"}`;
    return;
  }

  refs.domainCount.textContent = `${filteredCount} of ${count} domains`;
}

function renderDomains() {
  const query = state.domainFilter.trim().toLowerCase();
  const filtered = state.domains.filter((domain) => domain.toLowerCase().includes(query));

  updateDomainCountLabel(state.domains.length, filtered.length);

  if (filtered.length === 0) {
    const message = query ? "No matching domains found." : "No custom blocked domains yet.";
    refs.domainTableBody.innerHTML = `<tr><td colspan="2" class="placeholder">${message}</td></tr>`;
    return;
  }

  refs.domainTableBody.replaceChildren();

  for (const domain of filtered) {
    const row = document.createElement("tr");

    const domainCell = document.createElement("td");
    domainCell.textContent = domain;

    const actionCell = document.createElement("td");
    const removeButton = document.createElement("button");
    removeButton.className = "btn btn-danger";
    removeButton.type = "button";
    removeButton.dataset.removeDomain = domain;
    removeButton.textContent = "Unblock";
    actionCell.appendChild(removeButton);

    row.appendChild(domainCell);
    row.appendChild(actionCell);
    refs.domainTableBody.appendChild(row);
  }
}

function renderStats(stats) {
  refs.metrics.totalQueries.textContent = formatInt(stats.total_queries);
  refs.metrics.blockedQueries.textContent = formatInt(stats.blocked_queries);
  refs.metrics.blockRate.textContent = formatPercentage(stats.block_percentage);
  refs.metrics.cacheHits.textContent = formatInt(stats.cache_hits);
  refs.metrics.cacheMisses.textContent = formatInt(stats.cache_misses);
  refs.metrics.cacheHitRate.textContent = formatPercentage(stats.cache_hit_percentage);
  refs.metrics.latency.textContent = formatLatency(stats.average_upstream_latency_ms);
  refs.metrics.uptime.textContent = formatUptime(stats.uptime_seconds);
  refs.metrics.errors.textContent = formatInt(stats.upstream_errors);

  const now = new Date();
  refs.statsUpdatedAt.textContent = `Last updated: ${now.toLocaleString()}`;
}

async function loadHealth() {
  try {
    const response = await fetch(API.health, { method: "GET" });
    const text = (await response.text()).trim();

    if (response.ok) {
      setHealthStatus("ok", "API Healthy");
    } else {
      setHealthStatus("warning", `API ${response.status}`);
    }

    if (text && text !== "Admin API is running!") {
      showToast(text, "success");
    }
  } catch (error) {
    setHealthStatus("error", "API Unreachable");
  }
}

async function loadStats() {
  const stats = await apiRequest(API.stats);
  renderStats(stats);
}

async function loadDomains() {
  setLoadingDomains();
  const payload = await apiRequest(API.domains);
  state.domains = Array.isArray(payload.domains) ? payload.domains.slice().sort() : [];
  renderDomains();
}

async function addDomain(rawDomain) {
  const domain = normalizeDomain(rawDomain);

  if (!isLikelyDomain(domain)) {
    throw new Error("Enter a valid domain, such as example.com.");
  }

  const result = await apiRequest(API.domains, {
    method: "POST",
    body: JSON.stringify({ domain }),
  });

  return result;
}

async function removeDomain(domain) {
  const normalized = normalizeDomain(domain);

  const result = await apiRequest(API.domains, {
    method: "DELETE",
    body: JSON.stringify({ domain: normalized }),
  });

  return result;
}

async function refreshAll() {
  refs.refreshButton.disabled = true;

  try {
    await Promise.all([loadHealth(), loadStats(), loadDomains()]);
  } finally {
    refs.refreshButton.disabled = false;
  }
}

async function handleAddDomainSubmit(event) {
  event.preventDefault();

  const formData = new FormData(refs.addDomainForm);
  const domainInput = (formData.get("domain") || "").toString();

  try {
    const response = await addDomain(domainInput);
    showToast(response.message || "Domain added.", response.status === "success" ? "success" : "error");
    refs.domainInput.value = "";
    await loadDomains();
  } catch (error) {
    showToast(error.message, "error");
  }
}

async function handleDomainActionClick(event) {
  const removeButton = event.target.closest("[data-remove-domain]");
  if (!removeButton) {
    return;
  }

  const domain = removeButton.getAttribute("data-remove-domain");
  if (!domain) {
    return;
  }

  const approved = window.confirm(`Unblock ${domain}?`);
  if (!approved) {
    return;
  }

  removeButton.disabled = true;

  try {
    const response = await removeDomain(domain);
    const kind = response.status === "success" ? "success" : "error";
    showToast(response.message || "Domain removed.", kind);
    await loadDomains();
  } catch (error) {
    showToast(error.message, "error");
  } finally {
    removeButton.disabled = false;
  }
}

function registerEvents() {
  refs.refreshButton.addEventListener("click", () => {
    refreshAll().catch((error) => {
      showToast(error.message, "error");
    });
  });

  refs.addDomainForm.addEventListener("submit", (event) => {
    handleAddDomainSubmit(event).catch((error) => {
      showToast(error.message, "error");
    });
  });

  refs.domainSearch.addEventListener("input", (event) => {
    state.domainFilter = String(event.target.value || "");
    renderDomains();
  });

  refs.domainTableBody.addEventListener("click", (event) => {
    handleDomainActionClick(event).catch((error) => {
      showToast(error.message, "error");
    });
  });

  window.addEventListener("visibilitychange", () => {
    if (document.hidden) {
      return;
    }

    loadStats().catch(() => {});
  });
}

async function boot() {
  registerEvents();

  try {
    await refreshAll();
  } catch (error) {
    showToast(error.message, "error");
  }

  state.pollerId = window.setInterval(() => {
    loadStats().catch(() => {
      setHealthStatus("warning", "Stats Delayed");
    });
  }, 5000);
}

boot();