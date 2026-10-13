export interface Color {
  Brightness: number;
  Contrast: number;
  Gamma: number;
  Shadows: number;
  Highlights: number;
  RedGain: number;
  GreenGain: number;
  BlueGain: number;
  DigitalVibrance: number | null;
}
export interface Profile {
  Id: string;
  Name: string;
  IccPath: string | null;
  Color: Color;
  ProcessNames?: string[];
}
export interface Monitor {
  DeviceName: string;
  MonitorId: string;
  Label: string;
  Primary: boolean;
}
export interface Settings {
  SchemaVersion: number;
  Profiles: Profile[];
  SelectedProfileId: string | null;
  GameProfileId: string | null;
  MonitorId: string | null;
  FollowGame: boolean;
  GameProcesses: string[];
  PollSeconds: number;
  StartWithWindows: boolean;
  CloseToTray: boolean;
  RestoreOnExit: boolean;
  LiveAdjust: boolean;
}
export interface View {
  appVersion: string;
  settings: Settings;
  monitors: Monitor[];
  iccProfiles: string[];
  nvidiaPercent: number | null;
  nvidiaMessage: string;
  activeProfile: Profile | null;
  comparing: boolean;
  pendingRecovery: boolean;
  gameRunning: boolean;
  autoOwned: boolean;
  error: string | null;
  draft: Profile;
  hasReference: boolean;
  smoke: boolean;
  supported: boolean;
  startupActual: boolean;
  liveEnabled: boolean;
  liveVerifying: boolean;
}
export type Request =
  | { type: "Apply" | "SaveProfile"; profile: Profile }
  | { type: "Live"; profile: Profile }
  | { type: "SetLive"; enabled: boolean; profile: Profile }
  | { type: "Restore" | "Compare" | "Toggle" | "Refresh" }
  | { type: "Select" | "Delete"; id: string }
  | { type: "UpdateProfile"; id: string; name: string; processes: string[] }
  | { type: "Reorder"; ids: string[] }
  | { type: "Settings"; settings: Settings };
export const neutral: Color = {
  Brightness: 0,
  Contrast: 0,
  Gamma: 1,
  Shadows: 0,
  Highlights: 0,
  RedGain: 0,
  GreenGain: 0,
  BlueGain: 0,
  DigitalVibrance: null,
};
export function browserView(): View {
  const profiles: Profile[] = [
    ["均衡 · 三角洲", 1.1, 14, 8],
    ["室内 · 暗部可见", 1.18, 24, 10],
    ["室外 · 保留高光", 1.04, 8, 24],
    ["中性 · 校准基准", 1, 0, 0],
  ].map((p, i) => ({
    Id: (i + 1).toString().padStart(32, "0"),
    Name: String(p[0]),
    IccPath: null,
    Color: {
      ...neutral,
      Gamma: Number(p[1]),
      Shadows: Number(p[2]),
      Highlights: Number(p[3]),
    },
  }));
  return {
    appVersion: "0.2.1",
    settings: {
      SchemaVersion: 1,
      Profiles: profiles,
      SelectedProfileId: null,
      GameProfileId: null,
      MonitorId: null,
      FollowGame: false,
      GameProcesses: [
        "DeltaForceClient-Win64-Shipping.exe",
        "DeltaForceClient.exe",
      ],
      PollSeconds: 3,
      StartWithWindows: false,
      CloseToTray: true,
      RestoreOnExit: true,
      LiveAdjust: true,
    },
    monitors: [],
    iccProfiles: [],
    nvidiaPercent: null,
    nvidiaMessage: "在 Windows 客户端中检测 NVIDIA 输出",
    activeProfile: null,
    comparing: false,
    pendingRecovery: false,
    gameRunning: false,
    autoOwned: false,
    error: null,
    draft: structuredClone(profiles[0]),
    hasReference: false,
    smoke: false,
    supported: false,
    startupActual: false,
    liveEnabled: false,
    liveVerifying: false,
  };
}
export interface UpdateInfo {
  currentVersion: string;
  latestVersion: string;
  available: boolean;
  releaseUrl: string;
  notes: string;
}
export interface UpdateProgress {
  downloaded: number;
  total: number;
}
// Browser-only interface preview. The native client obtains this LUT from Rust, including ICC calibration.
export function previewLut(c: Color): number[] {
  const gains = [c.RedGain, c.GreenGain, c.BlueGain];
  return Array.from({ length: 768 }, (_, i) => {
    const clamp = (v: number) => Math.max(0, Math.min(1, v));
    let v = clamp(
      ((i % 256) / 255 - 0.5) * Math.pow(2, c.Contrast / 82) +
        0.5 +
        (c.Brightness / 125) * 0.15,
    );
    v = Math.pow(v, 1 / c.Gamma);
    v += (c.Shadows / 100) * 4 * v * Math.pow(1 - v, 3);
    let s = clamp((v - 0.55) / 0.45);
    s = s * s * (3 - 2 * s);
    v *= 1 - (c.Highlights / 100) * 0.35 * s;
    return Math.max(
      0,
      Math.min(
        65535,
        Math.round(
          clamp(v) * 65535 * (1 + (gains[Math.floor(i / 256)] / 100) * 0.48),
        ),
      ),
    );
  });
}
