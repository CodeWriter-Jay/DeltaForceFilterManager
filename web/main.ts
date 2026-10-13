import "./style.css";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  neutral,
  browserView,
  previewLut,
  type Color,
  type Profile,
  type Request,
  type View,
  type UpdateInfo,
  type UpdateProgress,
} from "./types";
const native = isTauri();
const $ = <T extends HTMLElement>(selector: string): T => {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error(`Missing element ${selector}`);
  return element;
};
let view: View;
let draft: Profile;
let busy = false;
let localBusy = false;
let externalOperations = 0;
let image: ImageData;
let lut: number[] = [];
let previewRevision = 0;
let previewTimer: number | undefined;
let toastTimer: number | undefined;
let livePending: Profile | null = null;
let liveFlight: Promise<void> | null = null;
let liveTimer: number | undefined;
let updateInfo: UpdateInfo | null = null;
let updateBusy = false;
let settingsProfilesSignature = "";
let managedProfileId: string | null = null;
let profileDrag: {
  id: string;
  pointerId: number;
  startY: number;
  targetId: string;
  before: boolean;
  moved: boolean;
} | null = null;
const clone = <T>(v: T): T => structuredClone(v);
const fields: [keyof Color, string, number, number, number, string][] = [
  ["Brightness", "亮度", -125, 125, 1, ""],
  ["Contrast", "对比度", -82, 82, 1, ""],
  ["Gamma", "Gamma", 0.4, 2.8, 0.01, ""],
  ["Shadows", "暗部提升", 0, 50, 1, "%"],
  ["Highlights", "高光压缩", 0, 50, 1, "%"],
  ["RedGain", "红色增益", -100, 100, 1, ""],
  ["GreenGain", "绿色增益", -100, 100, 1, ""],
  ["BlueGain", "蓝色增益", -100, 100, 1, ""],
  ["DigitalVibrance", "数字振动", 0, 100, 1, "%"],
];
function showError(error: unknown) {
  if (view) view.error = String(error);
  const box = $("#error");
  box.textContent = String(error);
  box.hidden = false;
}
function toast(text: string) {
  clearTimeout(toastTimer);
  const box = $("#toast");
  box.textContent = text;
  box.hidden = false;
  toastTimer = window.setTimeout(() => (box.hidden = true), 2500);
}
function savedProfile(): Profile {
  return (
    view.settings.Profiles.find((p) => p.Id === draft.Id) ??
    view.settings.Profiles[0]
  );
}
function dirty(): boolean {
  return JSON.stringify(draft) !== JSON.stringify(savedProfile());
}
function processNames(value: string): string[] {
  return value
    .split("\n")
    .map((name) => name.trim())
    .filter(Boolean);
}
function validateProcessNames(names: string[]) {
  if (names.length > 16)
    throw new Error("最多填写 16 个 .exe 文件名，不要填写路径");
  const unique = new Set<string>();
  for (const name of names) {
    if (
      !name.trim() ||
      name !== name.trim() ||
      Array.from(name).length > 200 ||
      /[\\/:*?"<>|\x00-\x1f\x7f]/.test(name) ||
      !name.toLowerCase().endsWith(".exe")
    )
      throw new Error("请填写 .exe 进程文件名，不要填写路径");
    if (unique.has(name.toLowerCase()))
      throw new Error("进程文件名重复（不区分大小写）");
    unique.add(name.toLowerCase());
  }
}
function validateMetadata(name: string, processes: string[]) {
  if (!name.trim() || Array.from(name).length > 80)
    throw new Error("方案名称不能为空，最多 80 个字符");
  validateProcessNames(processes);
}
function setProcesses(profile: Profile, processes: string[]) {
  if (processes.length) profile.ProcessNames = clone(processes);
  else delete profile.ProcessNames;
}
function setBusy(value: boolean) {
  localBusy = value;
  renderBusy();
}
function externalBusy(started: boolean) {
  externalOperations = Math.max(0, externalOperations + (started ? 1 : -1));
  renderBusy();
}
function renderBusy() {
  const value = localBusy || externalOperations > 0;
  busy = value;
  renderStatus();
  document
    .querySelectorAll<HTMLButtonElement>("#app button,#profile-dialog button")
    .forEach((b) => (b.disabled = value));
  document
    .querySelectorAll<
      HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement
    >(
      ".inspector input,.inspector select,#profile-name,#profile-dialog input,#profile-dialog textarea",
    )
    .forEach((e) => (e.disabled = value));
  if (!value) syncParameters();
  renderUpdate();
}
async function request(request: Request): Promise<View> {
  if (native) return invoke<View>("mutate", { request });
  const next = clone(view);
  next.error = null;
  switch (request.type) {
    case "SaveProfile": {
      validateMetadata(
        request.profile.Name,
        request.profile.ProcessNames ?? [],
      );
      const i = next.settings.Profiles.findIndex(
        (p) => p.Id === request.profile.Id,
      );
      if (i >= 0) next.settings.Profiles[i] = clone(request.profile);
      else next.settings.Profiles.push(clone(request.profile));
      next.settings.SelectedProfileId = request.profile.Id;
      next.draft = clone(request.profile);
      break;
    }
    case "UpdateProfile": {
      validateMetadata(request.name, request.processes);
      const profile = next.settings.Profiles.find((p) => p.Id === request.id);
      if (!profile) throw new Error("方案不存在");
      profile.Name = request.name;
      setProcesses(profile, request.processes);
      if (next.draft.Id === request.id) {
        next.draft.Name = request.name;
        setProcesses(next.draft, request.processes);
      }
      break;
    }
    case "Reorder": {
      if (
        request.ids.length !== next.settings.Profiles.length ||
        new Set(request.ids).size !== request.ids.length
      )
        throw new Error("排序必须包含所有方案且不能重复");
      next.settings.SelectedProfileId ??= next.settings.Profiles[0].Id;
      next.settings.Profiles = request.ids.map((id) => {
        const profile = next.settings.Profiles.find((p) => p.Id === id);
        if (!profile) throw new Error("方案不存在");
        return profile;
      });
      break;
    }
    case "Select":
      next.settings.SelectedProfileId = request.id;
      next.draft = clone(
        next.settings.Profiles.find((p) => p.Id === request.id) ??
          next.settings.Profiles[0],
      );
      break;
    case "Settings":
      next.settings = clone(request.settings);
      next.startupActual = request.settings.StartWithWindows;
      break;
    case "Delete":
      if (next.settings.Profiles.length === 1) {
        next.error = "至少保留一个方案";
        break;
      }
      next.settings.Profiles = next.settings.Profiles.filter(
        (p) => p.Id !== request.id,
      );
      if (next.settings.SelectedProfileId === request.id)
        next.settings.SelectedProfileId = null;
      if (next.settings.GameProfileId === request.id)
        next.settings.GameProfileId = null;
      if (next.draft.Id === request.id)
        next.draft = clone(
          next.settings.Profiles.find(
            (p) => p.Id === next.settings.SelectedProfileId,
          ) ?? next.settings.Profiles[0],
        );
      break;
    case "Restore":
      next.activeProfile = null;
      next.comparing = false;
      break;
    case "Apply":
    case "Toggle":
    case "Compare":
    case "Live":
    case "SetLive":
      next.error = "界面预览无法写入显示器，请使用 Windows 客户端";
      break;
    case "Refresh":
      break;
  }
  return next;
}
async function act(requestValue: Request, success?: string) {
  if (busy) return;
  setBusy(true);
  try {
    await stopLive(
      ["SaveProfile", "Settings", "UpdateProfile", "Reorder"].includes(
        requestValue.type,
      ),
    );
    const next = await request(requestValue);
    if (
      requestValue.type === "UpdateProfile" &&
      !next.error &&
      requestValue.id === draft.Id
    ) {
      draft.Name = requestValue.name;
      setProcesses(draft, requestValue.processes);
    }
    setView(
      next,
      !!next.error ||
        requestValue.type === "Settings" ||
        requestValue.type === "Refresh" ||
        requestValue.type === "UpdateProfile" ||
        requestValue.type === "Reorder" ||
        requestValue.type === "SetLive" ||
        (requestValue.type === "Delete" && requestValue.id !== draft.Id),
    );
    if (next.error) showError(next.error);
    else if (success) toast(success);
  } catch (e) {
    showError(e);
  } finally {
    setBusy(false);
  }
}
function setView(next: View, keepDraft = false) {
  view = next;
  if (!keepDraft) draft = clone(next.draft);
  renderProfiles();
  renderDevices();
  syncParameters();
  renderStatus();
  if ($("#settings-page").hidden || !keepDraft) renderSettings();
  else renderSettingsProfiles(true);
  schedulePreview();
  renderDisplayPreview();
  renderUpdate();
}
function renderProfiles() {
  clearProfileDrag();
  const box = $("#profiles");
  box.replaceChildren();
  for (const p of view.settings.Profiles) {
    const row = document.createElement("div");
    row.className = `profile-row ${p.Id === draft.Id ? "selected" : ""}`;
    row.dataset.profileId = p.Id;
    const handle = document.createElement("button");
    handle.className = "profile-drag";
    handle.textContent = "⋮⋮";
    handle.setAttribute("aria-label", `排序方案：${p.Name}`);
    handle.title = "拖动排序，或按 ↑ / ↓ 调整顺序";
    handle.onkeydown = (e) => {
      if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
      e.preventDefault();
      const index = view.settings.Profiles.findIndex(
        (profile) => profile.Id === p.Id,
      );
      const target =
        view.settings.Profiles[index + (e.key === "ArrowUp" ? -1 : 1)];
      if (target) void reorderProfile(p.Id, target.Id, e.key === "ArrowUp");
    };
    handle.onpointerdown = (e) => {
      if (busy || e.button !== 0) return;
      profileDrag = {
        id: p.Id,
        pointerId: e.pointerId,
        startY: e.clientY,
        targetId: p.Id,
        before: true,
        moved: false,
      };
      handle.setPointerCapture(e.pointerId);
    };
    handle.onpointermove = (e) => {
      if (
        !profileDrag ||
        profileDrag.pointerId !== e.pointerId ||
        (!profileDrag.moved && Math.abs(e.clientY - profileDrag.startY) < 4)
      )
        return;
      profileDrag.moved = true;
      const target = document
        .elementFromPoint(e.clientX, e.clientY)
        ?.closest<HTMLElement>(".profile-row");
      box
        .querySelectorAll(".drop-before,.drop-after")
        .forEach((r) => r.classList.remove("drop-before", "drop-after"));
      row.classList.add("dragging");
      profileDrag.targetId = target?.dataset.profileId ?? p.Id;
      if (target && target !== row) {
        profileDrag.before =
          e.clientY <
          target.getBoundingClientRect().top + target.offsetHeight / 2;
        target.classList.add(profileDrag.before ? "drop-before" : "drop-after");
      }
    };
    handle.onpointerup = (e) => {
      if (!profileDrag || profileDrag.pointerId !== e.pointerId) return;
      const { id, targetId, before } = profileDrag;
      clearProfileDrag();
      if (id !== targetId) void reorderProfile(id, targetId, before);
    };
    handle.onpointercancel = () => clearProfileDrag();
    handle.onlostpointercapture = () => clearProfileDrag();
    const b = document.createElement("button");
    b.className = "profile-card";
    b.setAttribute("aria-label", p.Name);
    b.title = p.Name;
    b.setAttribute("aria-pressed", String(p.Id === draft.Id));
    const title = document.createElement("strong");
    title.textContent = p.Name;
    const text = document.createElement("small");
    text.textContent = `Gamma ${p.Color.Gamma.toFixed(2)} · 暗部 ${p.Color.Shadows}`;
    b.append(title, text);
    if (p.ProcessNames?.length) {
      const binding = document.createElement("small");
      binding.textContent = `↔ ${p.ProcessNames.join("、")}`;
      binding.title = p.ProcessNames.join("\n");
      b.append(binding);
    }
    b.onclick = () => void switchProfile(p.Id);
    const manage = document.createElement("button");
    manage.className = "profile-options";
    manage.textContent = "···";
    manage.setAttribute("aria-label", `管理方案：${p.Name}`);
    manage.title = "重命名、绑定进程、删除";
    manage.onclick = () => editProfile(p.Id);
    row.append(handle, b, manage);
    box.append(row);
  }
  $("#manual-mode").classList.toggle("selected", !view.settings.FollowGame);
  $("#game-mode").classList.toggle("selected", view.settings.FollowGame);
  $("#manual-mode").setAttribute(
    "aria-pressed",
    String(!view.settings.FollowGame),
  );
  $("#game-mode").setAttribute(
    "aria-pressed",
    String(view.settings.FollowGame),
  );
  $("#game-hint").textContent = view.settings.FollowGame
    ? view.gameRunning
      ? "游戏运行中 · 退出后恢复"
      : `等待游戏 · 每 ${view.settings.PollSeconds} 秒检测`
    : "按需开启，后台不检测进程";
}
function clearProfileDrag() {
  profileDrag = null;
  $("#profiles")
    .querySelectorAll(".dragging,.drop-before,.drop-after")
    .forEach((row) =>
      row.classList.remove("dragging", "drop-before", "drop-after"),
    );
}
async function reorderProfile(id: string, targetId: string, before: boolean) {
  if (busy) return;
  const ids = view.settings.Profiles.map((p) => p.Id).filter(
    (value) => value !== id,
  );
  const index = ids.indexOf(targetId);
  if (index < 0) return;
  ids.splice(index + (before ? 0 : 1), 0, id);
  await act({ type: "Reorder", ids }, "方案顺序已保存");
  $("#profiles")
    .querySelector<HTMLElement>(`[data-profile-id="${id}"] .profile-drag`)
    ?.focus();
}
function editProfile(id: string) {
  if (busy) return;
  const profile = view.settings.Profiles.find((p) => p.Id === id);
  if (!profile) return;
  managedProfileId = id;
  $<HTMLInputElement>("#managed-profile-name").value = profile.Name;
  $<HTMLTextAreaElement>("#profile-processes").value = (
    profile.ProcessNames ?? []
  ).join("\n");
  $("#profile-message").hidden = true;
  $<HTMLDialogElement>("#profile-dialog").showModal();
}
async function deleteProfile(id: string) {
  if (busy) return;
  const profile = view.settings.Profiles.find((p) => p.Id === id);
  if (!profile) return;
  if (view.settings.Profiles.length === 1) {
    showError("至少保留一个方案");
    return;
  }
  if (
    (await choose("删除方案", `删除“${profile.Name}”？绑定进程也会移除。`, [
      ["delete", "删除"],
      ["cancel", "取消"],
    ])) === "delete"
  )
    await act({ type: "Delete", id }, "方案已删除");
}
function option(value: string, label: string) {
  const o = document.createElement("option");
  o.value = value;
  o.textContent = label;
  return o;
}
function renderDevices() {
  const monitor = $<HTMLSelectElement>("#monitor");
  monitor.replaceChildren(
    ...view.monitors.map((m) => option(m.MonitorId, m.Label)),
  );
  if (!view.monitors.length)
    monitor.append(option("", "未连接 Windows 显示器"));
  monitor.value = view.settings.MonitorId ?? view.monitors[0]?.MonitorId ?? "";
  const icc = $<HTMLSelectElement>("#icc");
  const paths = [...view.iccProfiles];
  if (draft.IccPath && !paths.includes(draft.IccPath))
    paths.push(draft.IccPath);
  icc.replaceChildren(
    option("", "原始显示器校准"),
    ...paths.map((p) => option(p, p.split(/[\\/]/).pop() ?? p)),
  );
  icc.value = draft.IccPath ?? "";
  $("#nvidia-status").textContent = view.nvidiaMessage;
  $("#preview-note").hidden = view.supported;
  $("#preview-note").textContent = view.smoke
    ? "CI 模拟模式 · 未调用真实显示器"
    : "界面预览 · 显示器调节需要在 Windows 客户端中运行";
  if (view.smoke) $("#preview-note").hidden = false;
}
function parameter(field: (typeof fields)[number]) {
  const [key, label, min, max, step, unit] = field;
  const container = document.createElement("div");
  container.className = "parameter";
  container.innerHTML = `<div class="parameter-header"><label for="${key}-range">${label}</label><div class="number-wrap"><input id="${key}-number" aria-label="${label}数值" type="number" min="${min}" max="${max}" step="${step}"><span>${unit}</span></div></div><input id="${key}-range" type="range" min="${min}" max="${max}" step="${step}"><div class="parameter-scale"><span>${min}</span><span>${max}</span></div>`;
  for (const input of container.querySelectorAll<HTMLInputElement>("input")) {
    const update = (event: Event) => {
      const value = Number(input.value);
      if (
        !input.value ||
        !Number.isFinite(value) ||
        value < min ||
        value > max
      ) {
        if (input.type === "number" && event.type === "input") return;
        input.value = String(draft.Color[key] ?? 50);
        showError(`${label}必须在 ${min}–${max} 之间`);
        return;
      }
      draft.Color[key] = Math.round(value / step) * step;
      syncParameters();
      onDraft(true);
    };
    input.addEventListener("input", update);
    if (input.type === "number") input.addEventListener("change", update);
  }
  return container;
}
function syncParameters() {
  for (const [key, , , max, ,] of fields) {
    const val =
      draft.Color[key] ??
      (key === "DigitalVibrance" ? (view.nvidiaPercent ?? 50) : max);
    for (const suffix of ["number", "range"]) {
      const input = $<HTMLInputElement>(`#${key}-${suffix}`);
      if (suffix !== "number" || document.activeElement !== input) {
        input.value = key === "Gamma" ? val.toFixed(2) : String(val);
      }
      $<HTMLInputElement>(`#${key}-${suffix}`).disabled =
        busy ||
        (key === "DigitalVibrance" &&
          (draft.Color.DigitalVibrance === null ||
            view.nvidiaPercent === null));
    }
  }
  $<HTMLInputElement>("#use-vibrance").checked =
    draft.Color.DigitalVibrance !== null;
  $<HTMLInputElement>("#use-vibrance").disabled =
    busy || view.nvidiaPercent === null;
  $<HTMLInputElement>("#profile-name").value = draft.Name;
  $<HTMLInputElement>("#live-adjust").checked = view.liveEnabled;
  $<HTMLInputElement>("#live-adjust").disabled = busy || !view.supported;
  $("#live-help").textContent = !view.supported
    ? "显示器实时调色需要 Windows 客户端。"
    : view.liveEnabled
      ? "调整参数会立即作用于显示器；方案需另行保存。"
      : "实时调色已暂停；开启后跟随参数变化，恢复按钮可还原显示器。";
  $<HTMLSelectElement>("#icc").value = draft.IccPath ?? "";
  $("#draft-state").textContent = dirty() ? "有未保存的调整" : "当前方案已保存";
}
function renderStatus() {
  const chip = $("#state-chip");
  chip.textContent = busy
    ? "正在应用与校验…"
    : view.pendingRecovery
      ? "恢复待完成"
      : view.comparing
        ? "原始对照中"
        : view.liveVerifying
          ? "实时调色 · 正在复查"
          : view.activeProfile
            ? "最近应用已校验"
            : "待启用";
  chip.className = `chip ${busy ? "busy" : view.activeProfile ? "active" : ""}`;
  $("#apply").innerHTML =
    `<span>◈</span> ${busy ? "正在校验色彩…" : view.activeProfile ? "应用当前调整" : "开启滤镜"}`;
  $("#footer-status").textContent = view.activeProfile
    ? `已应用：${view.activeProfile.Name}${view.autoOwned ? " · 游戏退出后恢复" : ""}`
    : view.comparing
      ? "正在显示原始色彩，点击开启可返回方案"
      : view.liveEnabled
        ? "调整参数会实时写入显示器"
        : "调整只在点击应用后写入显示器";
  const error = $("#error");
  error.hidden = !view.error;
  if (view.error) error.textContent = view.error;
}
function onDraft(colorChanged = false) {
  $("#draft-state").textContent = dirty() ? "有未保存的调整" : "当前方案已保存";
  schedulePreview();
  if (native)
    void invoke("remember_draft", { profile: clone(draft) }).catch(showError);
  if (colorChanged && view.liveEnabled && !busy) {
    livePending = clone(draft);
    if (liveTimer === undefined && !liveFlight) {
      liveTimer = window.setTimeout(() => {
        liveTimer = undefined;
        void flushLive();
      }, 50);
    }
  }
}
async function flushLive() {
  if (liveFlight || busy || !livePending || !view.liveEnabled) return;
  const profile = livePending;
  livePending = null;
  liveFlight = (async () => {
    try {
      const next = await request({ type: "Live", profile });
      setView(next, true);
      if (next.error) {
        livePending = null;
        showError(next.error);
      }
    } catch (error) {
      livePending = null;
      showError(error);
    }
  })();
  await liveFlight;
  liveFlight = null;
  if (livePending) void flushLive();
}
async function stopLive(flush = false) {
  clearTimeout(liveTimer);
  liveTimer = undefined;
  const pending = livePending;
  livePending = null;
  await liveFlight;
  if (flush && pending && view.liveEnabled) {
    const next = await request({ type: "Live", profile: pending });
    setView(next, true);
    if (next.error) throw new Error(next.error);
  }
}
function renderDisplayPreview() {
  const actualDisplay = native && (view.liveEnabled || !!view.activeProfile);
  $("#adjusted").hidden = actualDisplay;
  $("#divider").hidden = actualDisplay;
  $(".adjusted-label").hidden = actualDisplay;
  $(".split-control").hidden = actualDisplay;
  $(".original-label").textContent = actualDisplay ? "显示器实际效果" : "原始";
  $(".tip p").textContent = actualDisplay
    ? "直接观察显示器上的游戏与参考图。参考图不额外叠加滤镜，避免重复调色；点击保存方案保留参数。"
    : "先调 Gamma 与暗部，再看天空高光。参考图片只预览 RGB 曲线；数字振动请在显示器上对照。";
}
function renderUpdate() {
  $("#current-version").textContent = view.appVersion;
  $<HTMLButtonElement>("#check-update").disabled = busy || updateBusy;
  $<HTMLButtonElement>("#install-update").disabled = busy || updateBusy;
  $("#install-update").hidden = !updateInfo?.available;
}
async function checkUpdate() {
  if (busy || updateBusy) return;
  const status = $("#update-status");
  if (!native) {
    status.textContent = "请在 Windows 客户端中检查更新。";
    return;
  }
  updateBusy = true;
  updateInfo = null;
  $("#update-notes").hidden = true;
  $("#update-progress").hidden = true;
  status.textContent = "正在检查 GitHub 最新正式版本…";
  renderUpdate();
  try {
    updateInfo = await invoke<UpdateInfo>("check_update");
    status.textContent = updateInfo.available
      ? `发现新版本 ${updateInfo.latestVersion}，可以下载并重启升级。`
      : `当前 ${updateInfo.currentVersion}，最新正式版 ${updateInfo.latestVersion}，无需更新。`;
    $("#update-notes").textContent = updateInfo.notes;
    $("#update-notes").hidden = !updateInfo.notes;
  } catch (error) {
    status.textContent = `检查更新失败：${String(error)}`;
  } finally {
    updateBusy = false;
    renderUpdate();
  }
}
async function installUpdate() {
  if (busy || updateBusy || !updateInfo?.available) return;
  if (dirty()) {
    const choice = await choose(
      "升级前保存方案",
      "当前有未保存的参数。升级会恢复原始色彩并重启程序。",
      [
        ["save", "保存并升级"],
        ["discard", "放弃调整并升级"],
        ["cancel", "取消"],
      ],
    );
    if (choice === "save") {
      await act({ type: "SaveProfile", profile: clone(draft) });
      if (view.error) return;
    } else if (choice !== "discard") return;
  } else if (
    (await choose(
      "下载并重启升级",
      `升级到 ${updateInfo.latestVersion}？方案会沿用，升级前自动备份并恢复原始色彩。`,
      [
        ["install", "开始升级"],
        ["cancel", "取消"],
      ],
    )) !== "install"
  )
    return;
  updateBusy = true;
  setBusy(true);
  $("#update-progress").hidden = false;
  $<HTMLProgressElement>("#update-progress").value = 0;
  $("#update-status").textContent = "正在下载并校验更新…";
  try {
    await stopLive();
    await invoke("install_update");
    $("#update-status").textContent = "更新已准备完成，正在重启…";
  } catch (error) {
    try {
      setView(await invoke<View>("bootstrap"), true);
    } catch (refreshError) {
      showError(refreshError);
    }
    $("#update-status").textContent = `升级失败：${String(error)}`;
  } finally {
    updateBusy = false;
    setBusy(false);
  }
}
async function choose(
  title: string,
  message: string,
  choices: [string, string][],
): Promise<string> {
  const dialog = $<HTMLDialogElement>("#confirm-dialog");
  $("#dialog-title").textContent = title;
  $("#dialog-message").textContent = message;
  const box = $("#dialog-choices");
  box.replaceChildren();
  for (const [value, label] of choices) {
    const b = document.createElement("button");
    b.value = value;
    b.textContent = label;
    b.className = value === "cancel" ? "outline-button" : "primary-button";
    box.append(b);
  }
  dialog.returnValue = "";
  dialog.showModal();
  return new Promise((resolve) =>
    dialog.addEventListener("close", () => resolve(dialog.returnValue), {
      once: true,
    }),
  );
}
async function switchProfile(id: string) {
  if (id === draft.Id || busy) return;
  if (dirty()) {
    const choice = await choose(
      "切换色彩方案",
      "当前有未保存的调整。启用滤镜时，切换会立即应用目标方案的完整参数。",
      [
        ["save", "保存并切换"],
        ["discard", "放弃并切换"],
        ["cancel", "取消"],
      ],
    );
    if (choice === "save") {
      await act({ type: "SaveProfile", profile: clone(draft) });
      if (view.error) return;
    } else if (choice !== "discard") return;
  }
  await act(
    { type: "Select", id },
    view.activeProfile ? "方案已切换并应用" : undefined,
  );
}
function renderSettings() {
  $<HTMLInputElement>("#autostart").checked = view.startupActual;
  $<HTMLInputElement>("#close-tray").checked = view.settings.CloseToTray;
  $<HTMLInputElement>("#restore-exit").checked = view.settings.RestoreOnExit;
  $<HTMLTextAreaElement>("#process-names").value =
    view.settings.GameProcesses.join("\n");
  $<HTMLInputElement>("#poll-seconds").value = String(
    view.settings.PollSeconds,
  );
  renderSettingsProfiles(false);
}
function renderSettingsProfiles(preserve: boolean) {
  const signature = JSON.stringify(
    view.settings.Profiles.map((profile) => [profile.Id, profile.Name]),
  );
  if (preserve && signature === settingsProfilesSignature) return;
  const select = $<HTMLSelectElement>("#game-profile");
  const previous = select.value;
  select.replaceChildren(
    ...view.settings.Profiles.map((p) => option(p.Id, p.Name)),
  );
  select.value =
    preserve &&
    view.settings.Profiles.some((profile) => profile.Id === previous)
      ? previous
      : (view.settings.GameProfileId ?? draft.Id);
  settingsProfilesSignature = signature;
}
function page(settings: boolean) {
  $("#work-page").hidden = settings;
  $("#settings-page").hidden = !settings;
  $("#nav-work").classList.toggle("selected", !settings);
  $("#nav-settings").classList.toggle("selected", settings);
  $("#page-label").textContent = settings ? "应用设置" : "调色工作台";
  if (settings) renderSettings();
}
function schedulePreview() {
  clearTimeout(previewTimer);
  const revision = ++previewRevision;
  previewTimer = window.setTimeout(() => void updatePreview(revision), 90);
}
async function updatePreview(revision: number) {
  try {
    const profile = clone(draft);
    const next = native
      ? await invoke<number[]>("preview_curve", { profile })
      : previewLut(profile.Color);
    if (revision !== previewRevision) return;
    lut = next;
    drawCurve();
    if (!$("#adjusted").hidden) drawAdjusted();
  } catch (e) {
    showError(e);
  }
}
function drawCurve() {
  const canvas = $<HTMLCanvasElement>("#curve");
  const context = canvas.getContext("2d");
  if (!context) return;
  const w = canvas.width,
    h = canvas.height;
  context.clearRect(0, 0, w, h);
  context.strokeStyle = "#edf1f6";
  context.lineWidth = 1;
  for (let i = 0; i <= 4; i++) {
    context.beginPath();
    context.moveTo(18 + ((w - 36) * i) / 4, 8);
    context.lineTo(18 + ((w - 36) * i) / 4, h - 9);
    context.stroke();
    context.beginPath();
    context.moveTo(18, 8 + ((h - 17) * i) / 4);
    context.lineTo(w - 18, 8 + ((h - 17) * i) / 4);
    context.stroke();
  }
  context.setLineDash([4, 5]);
  context.strokeStyle = "#dce3ec";
  context.beginPath();
  context.moveTo(18, h - 9);
  context.lineTo(w - 18, 8);
  context.stroke();
  context.setLineDash([]);
  for (let ch = 0; ch < 3; ch++) {
    context.strokeStyle = ["#d99791", "#63b49e", "#859fce"][ch];
    context.lineWidth = 1.8;
    context.beginPath();
    for (let i = 0; i < 256; i++) {
      const x = 18 + (i / 255) * (w - 36),
        y = h - 9 - ((lut[ch * 256 + i] ?? 0) / 65535) * (h - 17);
      if (i) context.lineTo(x, y);
      else context.moveTo(x, y);
    }
    context.stroke();
  }
}
function drawAdjusted() {
  if (!image || lut.length !== 768) return;
  const adjusted = $<HTMLCanvasElement>("#adjusted");
  adjusted.width = image.width;
  adjusted.height = image.height;
  const context = adjusted.getContext("2d");
  if (!context) return;
  const result = new ImageData(
    new Uint8ClampedArray(image.data),
    image.width,
    image.height,
  );
  for (let i = 0; i < result.data.length; i += 4) {
    result.data[i] = Math.round(lut[result.data[i]] / 257);
    result.data[i + 1] = Math.round(lut[256 + result.data[i + 1]] / 257);
    result.data[i + 2] = Math.round(lut[512 + result.data[i + 2]] / 257);
  }
  context.putImageData(result, 0, 0);
}
function split(value: number) {
  const n = Math.min(100, Math.max(0, value));
  $<HTMLInputElement>("#split").value = String(n);
  $("#adjusted").style.clipPath = `inset(0 0 0 ${n}%)`;
  $("#divider").style.left = `${n}%`;
}
async function setImage(src: string, name: string) {
  const img = new Image();
  img.src = src;
  await img.decode();
  if (
    !img.naturalWidth ||
    !img.naturalHeight ||
    img.naturalWidth * img.naturalHeight > 32_000_000
  )
    throw new Error("图片超过 3200 万像素，请缩小后导入");
  const scale = Math.min(1, 1600 / img.naturalWidth, 900 / img.naturalHeight);
  const original = $<HTMLCanvasElement>("#original");
  original.width = Math.max(1, Math.round(img.naturalWidth * scale));
  original.height = Math.max(1, Math.round(img.naturalHeight * scale));
  const context = original.getContext("2d", { willReadFrequently: true });
  if (!context) throw new Error("无法创建图片预览");
  context.drawImage(img, 0, 0, original.width, original.height);
  image = context.getImageData(0, 0, original.width, original.height);
  $("#image-name").textContent = name;
  drawAdjusted();
}
function demoImage(): string {
  const c = document.createElement("canvas");
  c.width = 1280;
  c.height = 720;
  const x = c.getContext("2d");
  if (!x) throw new Error("Canvas 不可用");
  const sky = x.createLinearGradient(0, 0, 0, 720);
  sky.addColorStop(0, "#acbbc0");
  sky.addColorStop(0.48, "#a7ada5");
  sky.addColorStop(1, "#4c6067");
  x.fillStyle = sky;
  x.fillRect(0, 0, 1280, 720);
  // A locally drawn exposure chart: a dark hangar opening onto bright industrial terrain.
  x.fillStyle = "#6d7e7c";
  x.beginPath();
  x.moveTo(0, 365);
  x.lineTo(360, 330);
  x.lineTo(550, 373);
  x.lineTo(800, 306);
  x.lineTo(1100, 360);
  x.lineTo(1280, 300);
  x.lineTo(1280, 520);
  x.lineTo(0, 520);
  x.fill();
  x.fillStyle = "#87989a";
  x.fillRect(685, 330, 230, 85);
  x.fillStyle = "#acb5b2";
  x.fillRect(710, 312, 160, 18);
  x.fillStyle = "#607474";
  for (let i = 0; i < 5; i++) x.fillRect(700 + i * 39, 348, 26, 26);
  x.fillStyle = "#9b9b86";
  x.fillRect(980, 361, 220, 79);
  x.strokeStyle = "#767e6a";
  for (let i = 0; i < 15; i++) {
    x.beginPath();
    x.moveTo(987 + i * 14, 369);
    x.lineTo(987 + i * 14, 432);
    x.stroke();
  }
  const floor = x.createLinearGradient(0, 420, 0, 720);
  floor.addColorStop(0, "#627b7d");
  floor.addColorStop(1, "#2c3e45");
  x.fillStyle = floor;
  x.beginPath();
  x.moveTo(0, 500);
  x.lineTo(800, 400);
  x.lineTo(1280, 470);
  x.lineTo(1280, 720);
  x.lineTo(0, 720);
  x.fill();
  x.strokeStyle = "#a5b4a648";
  x.lineWidth = 2;
  for (let i = 0; i < 7; i++) {
    x.beginPath();
    x.moveTo(740, 424);
    x.lineTo(i * 240 - 150, 720);
    x.stroke();
  }
  for (const y of [483, 535, 610]) {
    x.beginPath();
    x.moveTo(0, y);
    x.lineTo(1280, y + 20);
    x.stroke();
  }
  x.fillStyle = "#182831";
  x.beginPath();
  x.moveTo(0, 0);
  x.lineTo(1280, 0);
  x.lineTo(1280, 195);
  x.lineTo(716, 112);
  x.lineTo(477, 276);
  x.lineTo(477, 530);
  x.lineTo(0, 670);
  x.fill();
  x.fillStyle = "#243640";
  x.beginPath();
  x.moveTo(0, 0);
  x.lineTo(305, 0);
  x.lineTo(423, 251);
  x.lineTo(423, 547);
  x.lineTo(0, 670);
  x.fill();
  x.strokeStyle = "#425861";
  x.lineWidth = 5;
  for (let i = 0; i < 5; i++) {
    x.beginPath();
    x.moveTo(20 + i * 82, 0);
    x.lineTo(62 + i * 82, 608 - i * 23);
    x.stroke();
  }
  x.fillStyle = "#304753";
  x.beginPath();
  x.moveTo(406, 212);
  x.lineTo(448, 228);
  x.lineTo(448, 552);
  x.lineTo(406, 567);
  x.fill();
  x.strokeStyle = "#344751";
  x.lineWidth = 20;
  x.beginPath();
  x.moveTo(40, 22);
  x.lineTo(675, 110);
  x.lineTo(1250, 155);
  x.stroke();
  x.fillStyle = "#283c43";
  x.fillRect(66, 472, 146, 111);
  x.fillStyle = "#354b52";
  x.beginPath();
  x.moveTo(66, 472);
  x.lineTo(128, 450);
  x.lineTo(269, 450);
  x.lineTo(212, 472);
  x.fill();
  x.fillStyle = "#20363f";
  x.beginPath();
  x.moveTo(212, 472);
  x.lineTo(269, 450);
  x.lineTo(269, 549);
  x.lineTo(212, 583);
  x.fill();
  x.strokeStyle = "#586b6640";
  x.lineWidth = 3;
  for (let i = 0; i < 6; i++) {
    x.beginPath();
    x.moveTo(78 + i * 22, 482);
    x.lineTo(78 + i * 22, 570);
    x.stroke();
  }
  x.fillStyle = "#bec5ab";
  x.fillRect(447, 239, 4, 75);
  x.fillStyle = "#cfbd75";
  x.fillRect(415, 423, 13, 53);
  x.fillStyle = "#40554f";
  x.fillRect(415, 440, 13, 10);
  x.fillRect(415, 460, 13, 10);
  x.strokeStyle = "#b9c3ad";
  x.lineWidth = 3;
  x.beginPath();
  x.moveTo(931, 242);
  x.lineTo(931, 425);
  x.stroke();
  x.lineWidth = 2;
  x.beginPath();
  x.moveTo(932, 251);
  x.lineTo(1280, 282);
  x.stroke();
  // Deterministic fine grain helps distinguish nearby dark tones without an external asset.
  const pixels = x.getImageData(0, 0, 1280, 720);
  for (let i = 0; i < pixels.data.length; i += 4) {
    const noise = (((i * 17) % 19) - 9) * 0.38;
    for (let j = 0; j < 3; j++) pixels.data[i + j] += noise;
  }
  x.putImageData(pixels, 0, 0);
  return c.toDataURL("image/png");
}
async function importImage() {
  try {
    if (native) {
      const data = await invoke<string | null>("reference_image", {
        pick: true,
        path: null,
      });
      if (data) await setImage(data, "本机参考图片");
    } else $<HTMLInputElement>("#image-file").click();
  } catch (e) {
    showError(e);
  }
}
async function fileAction(kind: string) {
  if (!native) {
    toast("ICC 和方案文件管理请在 Windows 客户端中使用");
    return;
  }
  setBusy(true);
  try {
    await stopLive(kind.startsWith("export"));
    const result = await invoke<{ profile: Profile | null } | null>(
      "file_action",
      {
        kind,
        profile: clone(draft),
      },
    );
    if (!result) return;
    const next = await invoke<View>("bootstrap");
    setView(next, kind.startsWith("export"));
    if (result.profile) draft = clone(result.profile);
    syncParameters();
    schedulePreview();
    if (kind === "import-icc" && view.liveEnabled) {
      const next = await request({ type: "Live", profile: clone(draft) });
      setView(next, true);
      if (next.error) showError(next.error);
    }
    if (kind.startsWith("export")) toast("已导出到选择的文件");
  } catch (e) {
    showError(e);
  } finally {
    setBusy(false);
  }
}
async function pickProcess(fieldSelector = "#process-names") {
  if (busy) return;
  try {
    const names = native
      ? await invoke<string[]>("running_processes")
      : ["DeltaForceClient.exe", "示例进程.exe"];
    const dialog = $<HTMLDialogElement>("#process-dialog");
    const render = (q: string) => {
      const list = $("#process-list");
      list.replaceChildren();
      for (const name of names.filter((n) =>
        n.toLowerCase().includes(q.toLowerCase()),
      )) {
        const b = document.createElement("button");
        b.textContent = name;
        b.onclick = () => {
          const field = $<HTMLTextAreaElement>(fieldSelector);
          const lines = processNames(field.value);
          if (!lines.some((line) => line.toLowerCase() === name.toLowerCase()))
            lines.push(name);
          field.value = lines.join("\n");
          dialog.close();
        };
        list.append(b);
      }
      if (!list.children.length) list.textContent = "未找到匹配进程";
    };
    render("");
    $<HTMLInputElement>("#process-search").value = "";
    $<HTMLInputElement>("#process-search").oninput = (e) =>
      render((e.currentTarget as HTMLInputElement).value);
    dialog.showModal();
  } catch (e) {
    showError(e);
  }
}
async function close() {
  if (native) {
    if (busy && !updateBusy) {
      toast("当前操作完成后可关闭窗口");
      return;
    }
    const wasBusy = localBusy;
    setBusy(true);
    try {
      await stopLive(true);
      await invoke("close_window", { profile: clone(draft) });
    } catch (e) {
      showError(e);
    } finally {
      setBusy(wasBusy);
    }
  } else toast("托盘功能在 Windows 客户端中可用");
}
async function smoke() {
  const checks: Record<string, boolean> = {};
  try {
    const a = clone(draft);
    a.Color = { ...a.Color, Brightness: 20, Gamma: 1.2, DigitalVibrance: 64 };
    await act({ type: "SaveProfile", profile: a });
    await act({ type: "Apply", profile: a });
    checks.apply = !view.error && view.activeProfile?.Color.Gamma === 1.2;
    await act({ type: "Select", id: view.settings.Profiles[1].Id });
    checks.switchB =
      !view.error && view.activeProfile?.Id === view.settings.Profiles[1].Id;
    await act({ type: "Select", id: a.Id });
    checks.switchBack =
      !view.error &&
      JSON.stringify(view.activeProfile?.Color) === JSON.stringify(a.Color);
    await updatePreview(++previewRevision);
    checks.referencePreview =
      image.width > 0 &&
      $("#adjusted").hidden &&
      $(".original-label").textContent === "显示器实际效果";
    await act({ type: "Compare" });
    checks.compare = view.comparing;
    await act({ type: "Apply", profile: a });
    await act({ type: "Restore" });
    checks.restore =
      !view.error && !view.activeProfile && !view.pendingRecovery;
    await act({
      type: "UpdateProfile",
      id: a.Id,
      name: `${a.Name} · 游戏绑定`,
      processes: ["GameA.exe", "GameLauncher.exe"],
    });
    const updated = view.settings.Profiles.find(
      (profile) => profile.Id === a.Id,
    );
    checks.profileMetadata =
      !view.error &&
      updated?.ProcessNames?.length === 2 &&
      updated.Name === `${a.Name} · 游戏绑定` &&
      draft.Color.Brightness === a.Color.Brightness;
    const ids = view.settings.Profiles.map((profile) => profile.Id);
    await act({ type: "Reorder", ids: [...ids].reverse() });
    checks.profileOrder =
      !view.error &&
      view.settings.Profiles[0].Id === ids.at(-1) &&
      view.settings.SelectedProfileId === a.Id &&
      draft.Id === a.Id;
    await act({ type: "Reorder", ids });
    await act({ type: "UpdateProfile", id: a.Id, name: a.Name, processes: [] });
    await act({ type: "SetLive", enabled: true, profile: clone(draft) });
    const live = clone(draft);
    live.Color.Brightness = 25;
    await act({ type: "Live", profile: live });
    checks.liveDisplay =
      !view.error &&
      view.activeProfile?.Color.Brightness === 25 &&
      view.liveVerifying;
    await act({ type: "Restore" });
    checks.liveRestore =
      !view.error && !view.liveEnabled && !view.activeProfile;
    await act({
      type: "Settings",
      settings: { ...view.settings, CloseToTray: true },
    });
    checks.settings = !view.error;
    await invoke("smoke_complete", {
      report: {
        passed: Object.values(checks).every(Boolean),
        checks,
        version: "0.2.1",
        simulation: true,
      },
    });
  } catch (e) {
    showError(e);
    await invoke("smoke_complete", {
      report: { passed: false, error: String(e), checks, simulation: true },
    });
  }
}
async function start() {
  try {
    view = native ? await invoke<View>("bootstrap") : browserView();
    draft = clone(view.draft);
    fields.forEach((field, i) =>
      $(
        i < 5
          ? "#tonal-parameters"
          : i < 8
            ? "#rgb-parameters"
            : "#vibrance-parameter",
      ).append(parameter(field)),
    );
    setView(view);
    await setImage(demoImage(), "内置明暗测试图");
    split(50);
    await updatePreview(++previewRevision);
    $("#boot").hidden = true;
    $("#app").hidden = false;
    $("#nav-work").onclick = () => page(false);
    $("#nav-settings").onclick = () => page(true);
    $(".brand").onclick = (e) => {
      e.preventDefault();
      page(false);
    };
    $("#apply").onclick = () =>
      void act({ type: "Apply", profile: clone(draft) });
    $<HTMLInputElement>("#live-adjust").onchange = (event) =>
      void act({
        type: "SetLive",
        enabled: (event.currentTarget as HTMLInputElement).checked,
        profile: clone(draft),
      });
    $("#restore").onclick = () => void act({ type: "Restore" });
    $("#compare").onclick = () =>
      void act(
        view.comparing
          ? { type: "Apply", profile: clone(draft) }
          : { type: "Compare" },
      );
    $("#save-profile").onclick = () =>
      void act({ type: "SaveProfile", profile: clone(draft) }, "方案已保存");
    $("#copy-profile").onclick = () => {
      const p = clone(draft);
      p.Id = crypto.randomUUID().replaceAll("-", "");
      p.Name = `${p.Name} · 副本`.slice(0, 80);
      void act({ type: "SaveProfile", profile: p }, "已另存为新方案");
    };
    $("#new-profile").onclick = () =>
      void act({
        type: "SaveProfile",
        profile: {
          Id: crypto.randomUUID().replaceAll("-", ""),
          Name: "我的方案",
          IccPath: null,
          Color: { ...neutral },
        },
      });
    $("#delete-profile").onclick = () => void deleteProfile(draft.Id);
    $("#cancel-profile-edit").onclick = () =>
      $<HTMLDialogElement>("#profile-dialog").close();
    $<HTMLDialogElement>("#profile-dialog").onclose = () => {
      const row = $("#profiles").querySelector<HTMLElement>(
        `[data-profile-id="${managedProfileId}"] .profile-options`,
      );
      (
        row ??
        $("#profiles").querySelector<HTMLElement>(".selected .profile-card")
      )?.focus();
      managedProfileId = null;
    };
    $("#profile-form").onsubmit = async (e) => {
      e.preventDefault();
      if (busy || !managedProfileId) return;
      const name = $<HTMLInputElement>("#managed-profile-name").value.trim();
      const processes = processNames(
        $<HTMLTextAreaElement>("#profile-processes").value,
      );
      const message = $("#profile-message");
      try {
        validateMetadata(name, processes);
        await act(
          { type: "UpdateProfile", id: managedProfileId, name, processes },
          "方案信息已保存",
        );
        if (view.error) throw new Error(view.error);
        $<HTMLDialogElement>("#profile-dialog").close();
      } catch (error) {
        message.textContent = String(error);
        message.hidden = false;
      }
    };
    $("#delete-managed-profile").onclick = async () => {
      if (!managedProfileId) return;
      await deleteProfile(managedProfileId);
      if (!view.settings.Profiles.some((p) => p.Id === managedProfileId))
        $<HTMLDialogElement>("#profile-dialog").close();
      else if (view.error) {
        $("#profile-message").textContent = view.error;
        $("#profile-message").hidden = false;
      }
    };
    $<HTMLInputElement>("#profile-name").oninput = (e) => {
      draft.Name = (e.currentTarget as HTMLInputElement).value;
      onDraft();
    };
    $("#reset-color").onclick = () => {
      draft.Color = { ...neutral };
      syncParameters();
      onDraft(true);
    };
    $<HTMLInputElement>("#use-vibrance").onchange = (e) => {
      draft.Color.DigitalVibrance = (e.currentTarget as HTMLInputElement)
        .checked
        ? (view.nvidiaPercent ?? 50)
        : null;
      syncParameters();
      onDraft(true);
    };
    $<HTMLSelectElement>("#monitor").onchange = (e) =>
      void act({
        type: "Settings",
        settings: {
          ...view.settings,
          MonitorId: (e.currentTarget as HTMLSelectElement).value || null,
        },
      });
    $<HTMLSelectElement>("#icc").onchange = (e) => {
      draft.IccPath = (e.currentTarget as HTMLSelectElement).value || null;
      onDraft(true);
    };
    $("#manual-mode").onclick = () =>
      void act({
        type: "Settings",
        settings: { ...view.settings, FollowGame: false },
      });
    $("#game-mode").onclick = () =>
      void act({
        type: "Settings",
        settings: { ...view.settings, FollowGame: true },
      });
    $("#upload-image").onclick = () => void importImage();
    $<HTMLInputElement>("#image-file").onchange = async (e) => {
      try {
        const file = (e.currentTarget as HTMLInputElement).files?.[0];
        if (!file) return;
        if (file.size > 25 * 1024 * 1024) throw new Error("图片最大 25 MB");
        const url = URL.createObjectURL(file);
        try {
          await setImage(url, file.name);
        } finally {
          URL.revokeObjectURL(url);
        }
      } catch (error) {
        showError(error);
      }
    };
    $<HTMLInputElement>("#split").oninput = (e) =>
      split(Number((e.currentTarget as HTMLInputElement).value));
    $("#reset-split").onclick = () => split(50);
    const stage = $("#image-stage");
    const move = (e: PointerEvent) => {
      const r = stage.getBoundingClientRect();
      split(((e.clientX - r.left) / r.width) * 100);
    };
    stage.onpointerdown = (e) => {
      stage.setPointerCapture(e.pointerId);
      move(e);
    };
    stage.onpointermove = (e) => {
      if (stage.hasPointerCapture(e.pointerId)) move(e);
    };
    stage.ondragover = (e) => {
      if (!native) {
        e.preventDefault();
        $("#drop-hint").hidden = false;
      }
    };
    stage.ondragleave = () => ($("#drop-hint").hidden = true);
    stage.ondrop = async (e) => {
      if (native) return;
      e.preventDefault();
      $("#drop-hint").hidden = true;
      const file = e.dataTransfer?.files[0];
      if (!file) return;
      try {
        if (file.size > 25 * 1024 * 1024) throw new Error("图片最大 25 MB");
        const url = URL.createObjectURL(file);
        try {
          await setImage(url, file.name);
        } finally {
          URL.revokeObjectURL(url);
        }
      } catch (error) {
        showError(error);
      }
    };
    for (const kind of [
      "import-profile",
      "export-profile",
      "import-icc",
      "export-icc",
    ])
      $(`#${kind}`).onclick = () => void fileAction(kind);
    $("#pick-process").onclick = () => void pickProcess();
    $("#pick-profile-process").onclick = () =>
      void pickProcess("#profile-processes");
    $("#tray").onclick = () => void close();
    $("#quit").onclick = () => {
      if (native) void invoke("quit").catch(showError);
      else toast("界面预览可直接关闭浏览器标签");
    };
    $("#refresh").onclick = () => void act({ type: "Refresh" });
    $("#check-update").onclick = () => void checkUpdate();
    $("#install-update").onclick = () => void installUpdate();
    $("#settings-form").onsubmit = async (e) => {
      e.preventDefault();
      const settings = {
        ...view.settings,
        StartWithWindows: $<HTMLInputElement>("#autostart").checked,
        CloseToTray: $<HTMLInputElement>("#close-tray").checked,
        RestoreOnExit: $<HTMLInputElement>("#restore-exit").checked,
        GameProcesses: processNames(
          $<HTMLTextAreaElement>("#process-names").value,
        ),
        GameProfileId: $<HTMLSelectElement>("#game-profile").value,
        PollSeconds: Number($<HTMLInputElement>("#poll-seconds").value),
      };
      if (
        !Number.isInteger(settings.PollSeconds) ||
        settings.PollSeconds < 2 ||
        settings.PollSeconds > 30
      ) {
        showError("检测间隔应为 2–30 秒的整数");
        return;
      }
      if (
        settings.GameProcesses.length > 16 ||
        settings.GameProcesses.some(
          (name) =>
            name.length > 200 ||
            /[\\/:*?"\x00]/.test(name) ||
            !name.toLowerCase().endsWith(".exe"),
        )
      ) {
        showError("请填写最多 16 个 .exe 文件名，不要填写路径");
        return;
      }
      await act({ type: "Settings", settings }, "设置已保存");
      if (!view.error) renderSettings();
      $("#settings-message").textContent = view.error ? "" : "设置已保存";
    };
    if (native) {
      await listen<View>("state", (e) => setView(e.payload, true));
      await listen<UpdateProgress>("update-progress", (event) => {
        const { downloaded, total } = event.payload;
        $<HTMLProgressElement>("#update-progress").value = total
          ? (downloaded / total) * 100
          : 0;
        $("#update-status").textContent =
          `正在下载 ${(downloaded / 1024 / 1024).toFixed(1)} / ${(total / 1024 / 1024).toFixed(1)} MB…`;
      });
      await listen("operation-start", () => externalBusy(true));
      await listen("operation-end", () => externalBusy(false));
      await listen("request-close", () => void close());
      await getCurrentWindow().onDragDropEvent(async (e) => {
        if (e.payload.type === "over") $("#drop-hint").hidden = false;
        else if (e.payload.type === "leave") $("#drop-hint").hidden = true;
        else if (e.payload.type === "drop") {
          $("#drop-hint").hidden = true;
          try {
            const data = await invoke<string | null>("reference_image", {
              pick: false,
              path: e.payload.paths[0],
            });
            if (data) await setImage(data, "本机参考图片");
          } catch (error) {
            showError(error);
          }
        }
      });
      if (view.hasReference) {
        const data = await invoke<string | null>("reference_image", {
          pick: false,
          path: null,
        });
        if (data) await setImage(data, "本机参考图片");
      }
      if (view.smoke) void smoke();
    }
  } catch (e) {
    $("#boot").textContent = `打开失败：${String(e)}`;
    showError(e);
  }
}
void start();
