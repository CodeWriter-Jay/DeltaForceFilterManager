import { test, expect, type Page } from "@playwright/test";
import {
  browserView,
  previewLut,
  type Request,
  type View,
  type Profile,
} from "../../web/types";

async function nativeSession(page: Page) {
  const initial = browserView();
  initial.supported = true;
  initial.liveEnabled = true;
  initial.monitors = [
    {
      DeviceName: "DISPLAY1",
      MonitorId: "test",
      Label: "测试显示器",
      Primary: true,
    },
  ];
  await page.addInitScript(
    ({ initial, lut }) => {
      const state: View = structuredClone(initial);
      const requests: Request[] = [];
      const commands: string[] = [];
      const closedDrafts: Profile[] = [];
      const callbacks = new Map<
        number,
        (event: { event: string; payload: unknown; id: number }) => void
      >();
      const listeners = new Map<string, number>();
      let nextId = 1;
      Object.assign(window, {
        isTauri: true,
        nativeRequests: requests,
        nativeCommands: commands,
        closedDrafts,
        __TAURI_INTERNALS__: {
          metadata: { currentWindow: { label: "main" } },
          transformCallback(
            callback: (event: {
              event: string;
              payload: unknown;
              id: number;
            }) => void,
          ) {
            const id = nextId++;
            callbacks.set(id, callback);
            return id;
          },
          unregisterCallback(id: number) {
            callbacks.delete(id);
          },
          async invoke(
            command: string,
            args: {
              request?: Request;
              profile?: Profile;
              event?: string;
              handler?: number;
            } = {},
          ) {
            commands.push(command);
            if (command === "bootstrap") return structuredClone(state);
            if (command === "preview_curve") return lut;
            if (
              command === "plugin:event|listen" &&
              args.event &&
              args.handler
            ) {
              listeners.set(args.event, args.handler);
              return nextId++;
            }
            if (command === "remember_draft" && args.profile) {
              state.draft = structuredClone(args.profile);
              return;
            }
            if (command === "close_window" && args.profile) {
              await new Promise((resolve) => setTimeout(resolve, 200));
              closedDrafts.push(structuredClone(args.profile));
              return;
            }
            if (command === "check_update")
              return {
                currentVersion: "0.2.1",
                latestVersion: "0.3.0",
                available: true,
                releaseUrl:
                  "https://github.com/CodeWriter-Jay/DeltaForceFilterManager/releases/tag/v0.3.0",
                notes: "更新说明",
              };
            if (command === "install_update") {
              const id = listeners.get("update-progress");
              if (id)
                callbacks.get(id)?.({
                  event: "update-progress",
                  id,
                  payload: { downloaded: 50, total: 100 },
                });
              await new Promise((resolve) => setTimeout(resolve, 100));
              throw new Error("下载中断，未替换 EXE");
            }
            if (command !== "mutate" || !args.request)
              throw new Error(`Unexpected command ${command}`);
            const request = args.request;
            requests.push(structuredClone(request));
            state.error = null;
            if (request.type === "Live") {
              await new Promise((resolve) => setTimeout(resolve, 120));
              state.activeProfile = structuredClone(request.profile);
              state.liveVerifying = true;
            } else if (
              request.type === "Restore" ||
              request.type === "Compare"
            ) {
              state.activeProfile = null;
              state.liveEnabled = false;
              state.liveVerifying = false;
              state.comparing = request.type === "Compare";
            } else if (request.type === "SetLive") {
              state.liveEnabled = request.enabled;
              state.settings.LiveAdjust = request.enabled;
              if (request.enabled)
                state.activeProfile = structuredClone(request.profile);
            } else if (request.type === "SaveProfile") {
              const index = state.settings.Profiles.findIndex(
                (profile) => profile.Id === request.profile.Id,
              );
              state.settings.Profiles[index] = structuredClone(request.profile);
              state.draft = structuredClone(request.profile);
            } else throw new Error(`Unexpected mutation ${request.type}`);
            return structuredClone(state);
          },
        },
      });
    },
    { initial, lut: previewLut(initial.draft.Color) },
  );
}

test("real-time IPC coalesces slider changes, keeps the final value and pauses after restore", async ({
  page,
}) => {
  await nativeSession(page);
  await page.goto("/");
  await expect(page.locator("#app")).toBeVisible();
  await expect(
    page.getByRole("switch", { name: "显示器实时调色" }),
  ).toBeChecked();
  await expect(page.locator("#adjusted")).not.toBeVisible();
  await expect(page.locator(".original-label")).toHaveText("显示器实际效果");
  const change = (value: number) =>
    page.locator("#Brightness-range").evaluate((element, value) => {
      (element as HTMLInputElement).value = String(value);
      element.dispatchEvent(new Event("input", { bubbles: true }));
    }, value);
  await change(10);
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (
            window as typeof window & { nativeRequests: Request[] }
          ).nativeRequests.filter((r) => r.type === "Live").length,
      ),
    )
    .toBe(1);
  await change(20);
  await change(30);
  await expect
    .poll(() =>
      page.evaluate(() => {
        const requests = (
          window as typeof window & { nativeRequests: Request[] }
        ).nativeRequests.filter((r) => r.type === "Live");
        const last = requests.at(-1);
        return last?.type === "Live" ? last.profile.Color.Brightness : null;
      }),
    )
    .toBe(30);
  await expect(
    page.getByRole("spinbutton", { name: "亮度数值", exact: true }),
  ).toHaveValue("30");
  await expect(
    page.getByRole("spinbutton", { name: "亮度数值", exact: true }),
  ).toBeEnabled();
  await expect(page.locator("#draft-state")).toHaveText("有未保存的调整");
  await change(40);
  await page.getByRole("button", { name: "恢复原始色彩", exact: true }).click();
  await expect(
    page.getByRole("switch", { name: "显示器实时调色" }),
  ).not.toBeChecked();
  await change(50);
  await page.getByRole("button", { name: "保存方案", exact: true }).click();
  const types = await page.evaluate(() =>
    (
      window as typeof window & { nativeRequests: Request[] }
    ).nativeRequests.map((r) => r.type),
  );
  expect(types.slice(types.indexOf("Restore") + 1)).not.toContain("Live");
  await expect(page.locator("#draft-state")).toHaveText("当前方案已保存");
});

test("continuous dragging writes the display during the gesture and preserves settings being edited", async ({
  page,
}) => {
  await nativeSession(page);
  await page.goto("/");
  await expect(page.locator("#app")).toBeVisible();
  const duringDrag = await page.evaluate(async () => {
    const slider =
      document.querySelector<HTMLInputElement>("#Brightness-range")!;
    let duringDrag = 0;
    for (let value = 1; value <= 16; value++) {
      slider.value = String(value);
      slider.dispatchEvent(new Event("input", { bubbles: true }));
      await new Promise((resolve) => setTimeout(resolve, 20));
      if (value === 8)
        duringDrag = (
          window as typeof window & { nativeRequests: Request[] }
        ).nativeRequests.filter((r) => r.type === "Live").length;
    }
    return duringDrag;
  });
  expect(duringDrag).toBeGreaterThan(0);
  await page.getByRole("button", { name: "应用设置", exact: true }).click();
  await page.locator("#process-names").fill("unsaved-game.exe");
  await expect
    .poll(() =>
      page.evaluate(() => {
        const last = (
          window as typeof window & { nativeRequests: Request[] }
        ).nativeRequests
          .filter((r) => r.type === "Live")
          .at(-1);
        return last?.type === "Live" ? last.profile.Color.Brightness : null;
      }),
    )
    .toBe(16);
  await page.getByRole("button", { name: "检查更新", exact: true }).click();
  await expect(page.locator("#process-names")).toHaveValue("unsaved-game.exe");
});

test("update UI shows the new release, saves drafts before upgrading and reports download failure", async ({
  page,
}) => {
  await nativeSession(page);
  await page.goto("/");
  await expect(page.locator("#app")).toBeVisible();
  await page
    .getByRole("spinbutton", { name: "亮度数值", exact: true })
    .fill("35");
  await page.getByRole("button", { name: "应用设置", exact: true }).click();
  await page.getByRole("button", { name: "检查更新", exact: true }).click();
  await expect(page.locator("#update-status")).toContainText(
    "发现新版本 0.3.0",
  );
  await expect(page.locator("#update-notes")).toHaveText("更新说明");
  await page
    .getByRole("button", { name: "下载并重启升级", exact: true })
    .click();
  await page.getByRole("button", { name: "取消", exact: true }).click();
  const commandCount = () =>
    page.evaluate(
      () =>
        (
          window as typeof window & { nativeCommands: string[] }
        ).nativeCommands.filter((command) => command === "install_update")
          .length,
    );
  expect(await commandCount()).toBe(0);
  await page
    .getByRole("button", { name: "下载并重启升级", exact: true })
    .click();
  await page.getByRole("button", { name: "保存并升级", exact: true }).click();
  await expect(page.locator("#update-status")).toContainText("下载中断");
  expect(await commandCount()).toBe(1);
  const saved = await page.evaluate(() =>
    (
      window as typeof window & { nativeRequests: Request[] }
    ).nativeRequests.find((r) => r.type === "SaveProfile"),
  );
  expect(saved?.type === "SaveProfile" && saved.profile.Color.Brightness).toBe(
    35,
  );
  await expect(
    page.getByRole("button", { name: "检查更新", exact: true }),
  ).toBeEnabled();
  await expect(page.locator("#update-progress")).toHaveJSProperty("value", 50);
});

test("closing to tray waits for the last live edit and keeps that draft", async ({
  page,
}) => {
  await nativeSession(page);
  await page.goto("/");
  await expect(page.locator("#app")).toBeVisible();
  await page.locator("#Brightness-range").evaluate((element) => {
    (element as HTMLInputElement).value = "42";
    element.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await page.getByRole("button", { name: "收起到托盘", exact: true }).click();
  await expect(page.locator("#Brightness-range")).toBeDisabled();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (
            window as typeof window & { closedDrafts: Profile[] }
          ).closedDrafts.at(-1)?.Color.Brightness,
      ),
    )
    .toBe(42);
  const requests = await page.evaluate(
    () =>
      (window as typeof window & { nativeRequests: Request[] }).nativeRequests,
  );
  const lastLive = requests.filter((request) => request.type === "Live").at(-1);
  expect(lastLive?.type === "Live" && lastLive.profile.Color.Brightness).toBe(
    42,
  );
});
