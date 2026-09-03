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
  dirty: false,
  newRuleId: 0,
};

// ---------- DOM ----------
const $ = (id) => document.getElementById(id);
const rulesBody = $("rulesBody");
const emptyMsg = $("emptyMsg");

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
    for (const r of state.rules) {
      rulesBody.appendChild(renderRow(r));
    }
  }
  $("autostartCheckbox").checked = !!state.start_with_windows;
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
  const tr = rulesBody.querySelector(`tr[data-id="${r.id}"]`);
  if (!tr) return;
  tr.innerHTML = "";

  const tdTrigger = document.createElement("td");
  const triggerInput = document.createElement("input");
  triggerInput.value = r.trigger;
  tdTrigger.appendChild(triggerInput);
  tr.appendChild(tdTrigger);

  const tdArrow = document.createElement("td");
  tdArrow.className = "arrow";
  tdArrow.textContent = "→";
  tr.appendChild(tdArrow);

  const tdRepl = document.createElement("td");
  const replInput = document.createElement("input");
  replInput.value = r.replacement;
  tdRepl.appendChild(replInput);
  tr.appendChild(tdRepl);

  const tdErr = document.createElement("td");
  tdErr.className = "edit-row";
  tdErr.colSpan = 4;
  tr.appendChild(tdErr);

  replInput.focus();
  replInput.select();

  const save = async () => {
    const trigger = triggerInput.value.trim();
    const replacement = replInput.value;
    const err = validateRule(trigger, replacement);
    if (err) {
      const e = document.createElement("div");
      e.className = "err";
      e.textContent = err;
      tdErr.replaceChildren(e);
      return;
    }
    try {
      if (r._draft) {
        r._draft = false; // guard against double-Enter / double-click re-submit
        // First save of a new rule: create it server-side. The backend
        // regenerates the id; adopt what it returns.
        const res = await invoke("add_rule", {
          rule: { id: "", trigger, replacement, enabled: true, created_at: "" },
        });
        state.rules = res.rules;
      } else {
        const updated = { ...r, trigger, replacement };
        const res = await invoke("update_rule", { id: r.id, rule: updated });
        state.rules = res.rules;
      }
      state.dirty = true;
      render();
    } catch (e) {
      const ee = document.createElement("div");
      ee.className = "err";
      ee.textContent = String(e);
      tdErr.replaceChildren(ee);
    }
  };
  const cancel = () => {
    if (r._draft) {
      state.rules = state.rules.filter((x) => x.id !== r.id);
    }
    render();
  };

  replInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") save();
    else if (e.key === "Escape") cancel();
  });
  triggerInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { e.preventDefault(); replInput.focus(); }
    else if (e.key === "Escape") cancel();
  });

  // also add Save/Cancel buttons in actions cell
  const tdActions = document.createElement("td");
  tdActions.className = "actions";
  const saveBtn = document.createElement("button");
  saveBtn.textContent = "Save";
  saveBtn.className = "primary";
  saveBtn.addEventListener("click", save);
  const cancelBtn = document.createElement("button");
  cancelBtn.textContent = "Cancel";
  cancelBtn.style.marginLeft = "4px";
  cancelBtn.addEventListener("click", cancel);
  tdActions.appendChild(saveBtn);
  tdActions.appendChild(cancelBtn);
  tr.appendChild(tdActions);
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
  // Open a local edit row. Nothing hits Rust until the user saves a
  // valid rule — strict validation forbids empty triggers, so an
  // optimistic empty create would always fail.
  const tempId = `__new_${state.newRuleId++}`;
  const draft = { id: tempId, trigger: "", replacement: "", enabled: true, _draft: true };
  state.rules.push(draft);
  render();
  const tr = rulesBody.querySelector(`tr[data-id="${tempId}"]`);
  if (tr) beginEdit(draft);
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
    });
    state.start_with_windows = res.start_with_windows;
    state.blacklist = res.blacklist;
    state.scoped_to = res.scoped_to;
    state.rules = res.rules;
    state.dirty = false;
    render();
  } catch (e) {
    alert(`Save failed: ${e}`);
  }
}

async function importJson() {
  // No native file picker for v1; ask user for a path.
  const path = prompt("Path to import from:");
  if (!path) return;
  try {
    const res = await invoke("import_json", { path });
    state.rules = res.rules;
    state.start_with_windows = res.start_with_windows;
    state.blacklist = res.blacklist;
    state.dirty = true;
    render();
  } catch (e) {
    alert(`Import failed: ${e}`);
  }
}

async function exportJson() {
  const path = prompt("Path to export to (leave blank for default):", "");
  try {
    await invoke("export_json", { path: path || null });
    alert("Exported.");
  } catch (e) {
    alert(`Export failed: ${e}`);
  }
}

// ---------- wire up ----------
$("addBtn").addEventListener("click", addRule);
$("importBtn").addEventListener("click", importJson);
$("exportBtn").addEventListener("click", exportJson);
$("saveBtn").addEventListener("click", saveAll);
$("autostartCheckbox").addEventListener("change", () => { state.dirty = true; render(); });
$("blacklistBox").addEventListener("input", () => { state.dirty = true; render(); });
$("scopedToBox").addEventListener("input", () => { state.dirty = true; render(); });

// ---------- load on startup ----------
(async () => {
  try {
    const cfg = await invoke("get_config");
    state.rules = cfg.rules ?? [];
    state.start_with_windows = cfg.start_with_windows ?? true;
    state.scoped_to = cfg.scoped_to ?? null;
  $("scopedToBox").value = (state.scoped_to ?? []).join("\n");
    state.dirty = false;
    render();
  } catch (e) {
    document.body.innerHTML = `<div class="empty">Failed to load config: ${escape(String(e))}</div>`;
  }
})();
