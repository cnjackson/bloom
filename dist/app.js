// Bloom — UI controller.
// Talks to Rust via Tauri's invoke() API. The Rust side exposes commands
// defined in src-tauri/src/lib.rs. State is held in `state` and re-fetched
// after every mutation to keep Rust the single source of truth.

const invoke = window.__TAURI_INTERNALS__?.invoke
  ?? ((name, args) => window.__TAURI__.core.invoke(name, args));

const state = {
  rules: [],
  start_with_windows: true,
  blacklist: [],
  scoped_to: null,
  theme: { mode: "dark" },
  show_debug_log: false,
  dirty: false,
  newRuleId: 0,
};

// ---------- DOM ----------
const $ = (id) => document.getElementById(id);
const rulesBody = $("rulesBody");
const emptyMsg = $("emptyMsg");

function applyTheme(mode) {
  const m = (mode === "light" || mode === "dark" || mode === "system") ? mode : "dark";
  document.documentElement.setAttribute("data-theme", m);
  state.theme.mode = m;
  // Reflect in the modal's radios (no-op if modal not yet built)
  for (const r of document.querySelectorAll('input[name="theme"]')) {
    r.checked = r.value === m;
  }
}

function openSettings() {
  $("settingsModal").hidden = false;
}

function closeSettings() {
  $("settingsModal").hidden = true;
}

async function saveTheme(mode) {
  applyTheme(mode);
  // Theme persists with the same save_all endpoint. Send a minimal payload
  // (current values) so the schema fields stay consistent on disk.
  try {
    const res = await invoke("save_all", {
      startWithWindows: $("autostartCheckbox").checked,
      blacklist: state.blacklist,
      scopedTo: state.scoped_to,
      theme: state.theme,
      showDebugLog: state.show_debug_log,
    });
    state.theme = res.theme;
  } catch (e) {
    console.warn("saveTheme:", e);
  }
}



// ---------- Console hook ----------
// Pipe console.log / console.warn / console.error to the in-page debug
// overlay (visible only when state.show_debug_log is true). The actual
// native console is preserved as a fallback.
(function patchConsole() {
  const origLog = console.log;
  const origWarn = console.warn;
  const origError = console.error;
  const fmt = (...args) => args.map(a => {
    if (typeof a === "string") return a;
    try { return JSON.stringify(a); } catch (_) { return String(a); }
  }).join(" ");
  console.log = (...a) => { try { _dbg(fmt(...a)); } catch(_) {} origLog(...a); };
  console.warn = (...a) => { try { _dbg("[warn] " + fmt(...a)); } catch(_) {} origWarn(...a); };
  console.error = (...a) => { try { _dbg("[error] " + fmt(...a)); } catch(_) {} origError(...a); };
})();

function _dbg(msg) {
  const el = document.getElementById("dbg");
  if (!el) return;
  const line = document.createElement("div");
  line.textContent = "[" + new Date().toISOString().slice(11,19) + "] " + msg;
  el.appendChild(line);
  // keep last 50 lines
  while (el.children.length > 50) el.removeChild(el.firstChild);
}
window._dbg = _dbg;

function escape(s) {
  return String(s ?? "").replace(/[&<>"']/g, (c) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  })[c]);
}

function render() {
  rulesBody.innerHTML = "";
  if (state.rules.length === 0) {
    emptyMsg.hidden = false;
  } else {
    emptyMsg.hidden = true;
    // Sort purely for display - the persisted order on disk stays
    // insertion order so reloading still gives the user what they typed.
    const rules = state.sort_dir === 'asc'
      ? [...state.rules].sort((a, b) => a.trigger.localeCompare(b.trigger, undefined, { sensitivity: 'base' }))
      : state.sort_dir === 'desc'
      ? [...state.rules].sort((a, b) => b.trigger.localeCompare(a.trigger, undefined, { sensitivity: 'base' }))
      : state.rules;
    for (const r of rules) {
      rulesBody.appendChild(renderRow(r));
    }
    // Update the sort indicator arrow on the header.
    const ind = $("triggerSortIndicator");
    if (ind) ind.textContent = state.sort_dir === 'asc' ? "▲" : state.sort_dir === 'desc' ? "▼" : "";
  }
  $("autostartCheckbox").checked = !!state.start_with_windows;
  $("debugLogCheckbox").checked = !!state.show_debug_log;
  // Never overwrite a focused input: it resets the caret and breaks editing.
  const bl = $("blacklistBox");
  const sl = $("scopedToBox");
  if (document.activeElement !== bl && document.activeElement !== $("autostartCheckbox") && document.activeElement !== sl) {
    bl.value = (state.blacklist ?? []).join("\n");
  }
  if (document.activeElement !== sl && document.activeElement !== $("autostartCheckbox") && document.activeElement !== bl) {
    sl.value = (state.scoped_to ?? []).join("\n");
  }
  $("statusRules").textContent = `${state.rules.length} rule${state.rules.length === 1 ? "" : "s"}`;
  $("saveBtn").disabled = !state.dirty;
  $("statusMsg").textContent = state.dirty ? "Modified" : "Saved";
  $("statusMsg").style.color = state.dirty ? "var(--fg)" : "var(--ok)";
  $("statusDot").className = state.dirty ? "dot dirty" : "dot";
}

function renderRow(r) {
  const tr = document.createElement("tr");
  tr.dataset.id = r.id;
  if (!r.enabled) tr.classList.add("disabled");

  // trigger cell
  const tdTrigger = document.createElement("td");
  tdTrigger.className = "trigger";
  tdTrigger.textContent = r.trigger;
  tr.appendChild(tdTrigger);

  // arrow
  const tdArrow = document.createElement("td");
  tdArrow.className = "arrow";
  tdArrow.textContent = "→";
  tr.appendChild(tdArrow);

  // replacement cell (preview newline escapes)
  const tdRepl = document.createElement("td");
  tdRepl.className = "replacement";
  const preview = r.replacement.length > 60
    ? r.replacement.slice(0, 57).replace(/\n/g, "↵") + "…"
    : r.replacement.replace(/\n/g, "↵ ");
  tdRepl.textContent = preview;
  if (r.replacement.length > 60) {
    tdRepl.title = r.replacement.replace(/\n/g, "\n");
  }
  tr.appendChild(tdRepl);

  // spacer
  const tdSpacer1 = document.createElement("td");
  tr.appendChild(tdSpacer1);

  // enabled cell
  const tdEnabled = document.createElement("td");
  const cb = document.createElement("input");
  cb.type = "checkbox";
  cb.checked = !!r.enabled;
  cb.addEventListener("change", async () => {
    const updated = { ...r, enabled: cb.checked };
    await invoke("update_rule", { id: r.id, rule: updated });
    r.enabled = cb.checked;
    tr.classList.toggle("disabled", !cb.checked);
    state.dirty = true;
    render();
  });
  tdEnabled.appendChild(cb);
  tr.appendChild(tdEnabled);

  // spacer
  const tdSpacer2 = document.createElement("td");
  tr.appendChild(tdSpacer2);

  // actions
  const tdActions = document.createElement("td");
  tdActions.className = "actions";
  const editBtn = document.createElement("button");
  editBtn.textContent = "Edit";
  editBtn.addEventListener("click", () => beginEdit(r));
  const delBtn = document.createElement("button");
  delBtn.textContent = "✕";
  delBtn.className = "danger";
  delBtn.style.marginLeft = "4px";
  delBtn.addEventListener("click", () => deleteRule(r));
  tdActions.appendChild(editBtn);
  tdActions.appendChild(delBtn);
  tr.appendChild(tdActions);

  return tr;
}

function beginEdit(r) {
  // Open the Edit Rule modal pre-populated with this rule's data.
  openEditRule(r);
}

// ---------- Edit Rule modal ----------
let _editState = null;
function openEditRule(r) {
  _editState = { id: r.id };
  $("editRuleTitle").textContent = "Edit rule";
  $("editRuleTrigger").value = r.trigger;
  $("editRuleReplacement").value = r.replacement;
  $("editRuleEnabled").checked = !!r.enabled;
  $("editRuleModal").hidden = false;
  // Focus the textarea so the cursor lands in the big box immediately.
  setTimeout(() => {
    const ta = $("editRuleReplacement");
    ta.focus();
    ta.setSelectionRange(ta.value.length, ta.value.length);
  }, 30);
}

async function applyEditRule() {
  const s = _editState;
  if (!s) return;
  const trigger = $("editRuleTrigger").value.trim();
  const replacement = $("editRuleReplacement").value;
  const enabled = $("editRuleEnabled").checked;
  if (!trigger) {
    alert("Trigger cannot be empty.");
    $("editRuleTrigger").focus();
    return;
  }
  if (replacement.length > 4096) {
    alert(`Replacement too long (${replacement.length} / 4096 chars).`);
    return;
  }
  try {
    if (s.id) {
      const updated = {
        id: s.id,
        trigger, replacement, enabled,
        created_at: (state.rules.find((x) => x.id === s.id) || {}).created_at || "",
      };
      const res = await invoke("update_rule", { id: s.id, rule: updated });
      state.rules = res.rules ?? state.rules;
    } else {
      const res = await invoke("add_rule", {
        rule: { id: "", trigger, replacement, enabled, created_at: "" },
      });
      state.rules = res.rules ?? state.rules;
    }
    state.dirty = true;
    cancelEditRule();
    render();
  } catch (e) {
    alert(`Save failed:\n${e}`);
  }
}

function cancelEditRule() {
  $("editRuleModal").hidden = true;
  _editState = null;
}

function validateRule(trigger, replacement) {
  if (!trigger) return "Trigger cannot be empty.";
  // Multi-word triggers allowed (Mac parity); normalized by the backend
  // to trimmed + single internal spaces.
  if (trigger.length > 32) return "Trigger too long (max 32).";
  if (replacement.length > 4096) return "Replacement too long (max 4096).";
  return null;
}

async function deleteRule(r) {
  if (!confirm(`Delete rule "${r.trigger}"?`)) return;
  try {
    const res = await invoke("delete_rule", { id: r.id });
    state.rules = res.rules;
    state.dirty = true;
    render();
  } catch (e) {
    alert(`Delete failed: ${e}`);
  }
}

function addRule() {
  // Open the Edit Rule modal in "new rule" mode (no id -> backend uses
  // add_rule when the user clicks Save; update_rule otherwise).
  openEditRule({
    id: "",
    trigger: "",
    replacement: "",
    enabled: true,
  });
}

async function saveAll() {
  const autostart = $("autostartCheckbox").checked;
  const blacklist = $("blacklistBox").value.split(/\r?\n/).map(s => s.trim()).filter(Boolean);
  const scopedToText = $("scopedToBox").value.trim();
  // Empty box = None (expand everywhere); non-empty = explicit list
  const scopedTo = scopedToText ? scopedToText.split(/\r?\n/).map(s => s.trim()).filter(Boolean) : null;
  try {
        const res = await invoke("save_all", {
          startWithWindows: autostart,
          blacklist,
          scopedTo,
          theme: state.theme,
          showDebugLog: state.show_debug_log,
        });
      state.start_with_windows = res.start_with_windows;
      state.blacklist = res.blacklist;
      state.scoped_to = res.scoped_to;
      state.theme = res.theme;
      state.rules = res.rules;
    state.dirty = false;
    render();
  } catch (e) {
    alert(`Save failed: ${e}`);
  }
}

async function importJson() {
  _dbg('importJson() entered');
  // Native file picker via tauri-plugin-dialog, invoked directly through
  // __TAURI_INTERNALS__ so we don't need the plugin's JS wrapper to be
  // loaded. The Rust side handles the dialog.
  // Resolve the Downloads folder so the dialog defaults there.
  let dl = "";
  try {
    dl = (await window.__TAURI_INTERNALS__.invoke(
      "plugin:path|download_dir", {}
    )) || "";
  } catch (_) { /* non-fatal — dialog still opens, just defaults elsewhere */ }

  let path;
  try {
    const selected = await window.__TAURI_INTERNALS__.invoke(
      "plugin:dialog|open",
      {
        options: {
          multiple: false,
          directory: false,
          filters: [{ name: "Bloom rules", extensions: ["json"] }],
          defaultPath: (dl || "") + "\\bloom-rules.json",
        },
      }
    );
    if (!selected || (typeof selected === "string" && !selected.trim())) return;
    path = typeof selected === "string" ? selected : String(selected || "").trim();
    if (!path) return;
  } catch (e) {
    _dbg("dialog error: " + e.message); console.warn("import dialog:", e);
    return;
  }
  // Read the file via tauri-plugin-fs (returns string when text mode)
  let raw;
  try {
    _dbg("reading file via plugin:fs...");
    raw = await window.__TAURI_INTERNALS__.invoke(
      "plugin:fs|read_text_file",
      { path }
    );
  } catch (e) {
    alert(
      `Import failed:\n${e}\n\n` +
      `Check the path is a valid JSON file and matches the Bloom rules schema.`
    );
    return;
  }
  if (raw instanceof ArrayBuffer || raw instanceof Uint8Array) {
    raw = new TextDecoder("utf-8").decode(raw);
  }
  let incoming;
  try { incoming = JSON.parse(raw); }
  catch (e) {
    alert(`Import failed: file is not valid JSON.\n${e}`);
    return;
  }
  if (!incoming || !Array.isArray(incoming.rules)) {
    alert("Import failed: file is not a Bloom rules.json (missing 'rules' array).");
    return;
  }
  const existing = state.rules || [];
  const existing_lc = new Map(existing.map((r) => [r.trigger.toLowerCase(), r]));
  const incoming_rules = incoming.rules;
  const conflicts = [];
  const seen = new Set();
  for (const ir of incoming_rules) {
    const key = (ir.trigger || "").toLowerCase();
    if (!key) continue;
    if (existing_lc.has(key) && !seen.has(key)) {
      conflicts.push({
        trigger: ir.trigger,
        existing: existing_lc.get(key),
        incoming: ir,
      });
      seen.add(key);
    }
  }
  if (conflicts.length === 0) {
    try {
      const res = await invoke("merge_import", { path, decisions: {} });
      applyImportedConfig(res);
      toast(`Imported ${incoming_rules.length} rule(s). Nothing conflicted.`);
    } catch (e) {
      alert(`Import failed:\n${e}`);
    }
    return;
  }
  showImportConflicts(path, incoming_rules.length, conflicts);
}

// Open the conflict modal. decisions[i].overwrite = true means overwrite
// the existing rule at conflicts[i].trigger; false = skip.
let _importState = null;
function showImportConflicts(path, incomingCount, conflicts) {
  _importState = { path, incomingCount, conflicts, decisions: [] };
  const tbody = $("importConflictRows");
  tbody.innerHTML = "";
  for (let i = 0; i < conflicts.length; i++) {
    const c = conflicts[i];
    const tr = document.createElement("tr");
    tr.dataset.idx = String(i);
    // Checkbox column
    const tdCb = document.createElement("td");
    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.id = `conflict_${i}`;
    cb.addEventListener("change", () => {
      _importState.decisions[i] = cb.checked ? "overwrite" : "skip";
    });
    const lbl = document.createElement("label");
    lbl.htmlFor = cb.id;
    lbl.style.marginLeft = "6px";
    lbl.textContent = "Overwrite";
    tdCb.appendChild(cb);
    tdCb.appendChild(lbl);
    tr.appendChild(tdCb);
    // Trigger column
    const tdT = document.createElement("td");
    tdT.style.fontFamily = "var(--mono, monospace)";
    tdT.textContent = c.trigger;
    tr.appendChild(tdT);
    // Current replacement
    const tdCur = document.createElement("td");
    tdCur.textContent = c.existing.replacement.length > 80
      ? c.existing.replacement.slice(0, 80) + "..."
      : c.existing.replacement;
    tdCur.style.color = "var(--muted)";
    tr.appendChild(tdCur);
    // Incoming replacement
    const tdNew = document.createElement("td");
    tdNew.textContent = c.incoming.replacement.length > 80
      ? c.incoming.replacement.slice(0, 80) + "..."
      : c.incoming.replacement;
    tdNew.style.color = "var(--muted)";
    tr.appendChild(tdNew);
    tbody.appendChild(tr);
    _importState.decisions[i] = "skip"; // default
  }
  $("importCount").textContent = String(incomingCount);
  $("importConflictCount").textContent = String(conflicts.length);
  $("importConflictsModal").hidden = false;
}

async function applyImportConflicts() {
  const s = _importState;
  if (!s) return;
  // Build the decisions map: trigger (case-preserved) -> decision
  const decMap = {};
  for (let i = 0; i < s.conflicts.length; i++) {
    decMap[s.conflicts[i].trigger] = s.decisions[i] || "skip";
  }
  $("importConflictsModal").hidden = true;
  try {
    const res = await invoke("merge_import", {
      path: s.path,
      decisions: decMap,
    });
    applyImportedConfig(res);
    let msg = `Imported ${s.incomingCount} rule(s).`;
    const overwritten = Object.values(decMap).filter((v) => v === "overwrite").length;
    const skipped = Object.values(decMap).filter((v) => v === "skip").length;
    if (skipped || overwritten) {
      msg += ` ${overwritten} overwritten, ${skipped} skipped.`;
    }
    toast(msg);
  } catch (e) {
    alert(`Import failed:\n${e}`);
  } finally {
    _importState = null;
  }
}

function cancelImportConflicts() {
  $("importConflictsModal").hidden = true;
  _importState = null;
}

function applyImportedConfig(res) {
  state.rules = res.rules ?? [];
  state.start_with_windows = res.start_with_windows ?? true;
  state.blacklist = res.blacklist ?? [];
  state.scoped_to = res.scoped_to ?? null;
  state.theme = res.theme ?? { mode: "dark" };
  state.dirty = true;
  render();
}

// Tiny toast: 3 seconds, fade.
let _toastTimer = null;

// Show/hide the debug overlay based on state.show_debug_log.
function applyDebugLogVisibility() {
  const el = document.getElementById("dbg");
  if (!el) return;
  el.style.display = state.show_debug_log ? "block" : "none";
}

function toast(msg) {
  const t = $("toast");
  t.textContent = msg;
  t.hidden = false;
  if (_toastTimer) clearTimeout(_toastTimer);
  _toastTimer = setTimeout(() => { t.hidden = true; }, 3000);
}



async function exportJson() {
  _dbg('exportJson() entered');
  let path;
  try {
    const dl = await window.__TAURI_INTERNALS__.invoke(
      "plugin:path|download_dir",
      {}
    ).catch(() => null);
    const suggested = (dl ? dl + "\\" : "") + "bloom-rules.json";
    path = await window.__TAURI_INTERNALS__.invoke(
      "plugin:dialog|save",
      { options: { defaultPath: suggested, filters: [{ name: "Bloom rules", extensions: ["json"] }] } }
    );
    if (!path) return;
  } catch (e) {
    _dbg("export dialog error: " + e.message); console.warn("export dialog:", e);
    return;
  }
  try {
    const target = await invoke("export_json", { path });
    toast(`Exported to ${target}`);
  } catch (e) {
    alert(`Export failed:\n${e}`);
  }
}



// ---------- sort ----------
$("triggerSortHeader").addEventListener("click", () => {
  // null -> asc -> desc -> null
  if (state.sort_dir === null || state.sort_dir === undefined) state.sort_dir = "asc";
  else if (state.sort_dir === "asc") state.sort_dir = "desc";
  else state.sort_dir = null;
  render();
});

// ---------- wire up ----------
$("addBtn").addEventListener("click", addRule);
$("importBtn").addEventListener("click", importJson);
$("exportBtn").addEventListener("click", exportJson);
$("saveBtn").addEventListener("click", saveAll);
$("settingsBtn").addEventListener("click", openSettings);
$("settingsClose").addEventListener("click", closeSettings);
$("settingsModal").addEventListener("click", (e) => {
  if (e.target === $("settingsModal")) closeSettings(); // backdrop click
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !$("settingsModal").hidden) closeSettings();
});
for (const r of document.querySelectorAll('input[name="theme"]')) {
  r.addEventListener("change", (e) => saveTheme(e.target.value));
}
$("autostartCheckbox").addEventListener("change", () => { state.dirty = true; render(); });
$("debugLogCheckbox").addEventListener("change", () => { state.show_debug_log = $("debugLogCheckbox").checked; state.dirty = true; render(); });
$("blacklistBox").addEventListener("input", () => { state.dirty = true; render(); });
$("scopedToBox").addEventListener("input", () => { state.dirty = true; render(); });

// ---------- Splash ----------
// Shows on first load only (sessionStorage flag). After 3s, the splash
// element is removed entirely. The rules window is already visible under it
// (auto-shown in tauri::Builder.setup()), so removing the splash surfaces
// the rules UI in place — no click needed.
(function setupSplash() {
  const seen = sessionStorage.getItem("bloom.splashSeen");
  const splash = $("splash");
  if (seen) {
    splash.remove();
    return;
  }
  splash.hidden = false;
  sessionStorage.setItem("bloom.splashSeen", "1");
  setTimeout(() => splash.remove(), 3000);
})();

// ---------- Settings tabs ----------
// Two tabs: Settings + About. Switch by clicking, support arrow keys.
const switchTab = (target) => {
  for (const t of document.querySelectorAll(".modal-tab")) {
    const sel = t.dataset.tab === target;
    t.setAttribute("aria-selected", sel ? "true" : "false");
  }
  for (const p of document.querySelectorAll(".settings-section[data-panel]")) {
    p.hidden = p.dataset.panel !== target;
  }
};
for (const t of document.querySelectorAll(".modal-tab")) {
  t.addEventListener("click", () => switchTab(t.dataset.tab));
}

// ---------- About panel ----------
// Populate the install-location + config-path rows from Rust on modal open.
async function populateAbout() {
  try {
    const meta = await invoke("get_app_meta");
    if (meta?.install_dir) $("aboutInstall").textContent = meta.install_dir;
    if (meta?.config_path) $("aboutConfig").textContent = meta.config_path;
  } catch (e) {
    // non-fatal; user just sees blanks
  }
}
// Hook into the settings button
$("settingsBtn").addEventListener("click", () => {
  switchTab("settings");
  populateAbout();
});

// ---------- Edit Rule modal wiring ----------
$("editRuleClose").addEventListener("click", cancelEditRule);
$("editRuleCancel").addEventListener("click", cancelEditRule);
$("editRuleApply").addEventListener("click", applyEditRule);
$("editRuleModal").addEventListener("click", (e) => {
  if (e.target === $("editRuleModal")) cancelEditRule();
});

// ---------- Import conflicts modal wiring ----------
$("importConflictsClose").addEventListener("click", cancelImportConflicts);
$("importConflictsCancel").addEventListener("click", cancelImportConflicts);
$("importConflictsApply").addEventListener("click", applyImportConflicts);
$("importConflictsModal").addEventListener("click", (e) => {
  if (e.target === $("importConflictsModal")) cancelImportConflicts(); // backdrop
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !$("editRuleModal").hidden) {
    cancelEditRule();
    return;
  }
  if (e.key === "Escape" && !$("importConflictsModal").hidden) {
    cancelImportConflicts();
  }
});

// ---------- load on startup ----------
(async () => {
  try {
    const cfg = await invoke("get_config");
    state.rules = cfg.rules ?? [];
    state.start_with_windows = cfg.start_with_windows ?? true;
    state.scoped_to = cfg.scoped_to ?? null;
    state.theme = cfg.theme ?? { mode: "dark" };
    state.show_debug_log = cfg.show_debug_log ?? false;
  $("scopedToBox").value = (state.scoped_to ?? []).join("\n");
    applyTheme(state.theme.mode); // apply BEFORE render so colors load first
    state.dirty = false;
    render();
  } catch (e) {
    document.body.innerHTML = `<div class="empty">Failed to load config: ${escape(String(e))}</div>`;
  }
})();
