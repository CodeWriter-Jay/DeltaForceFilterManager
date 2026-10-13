import { test, expect } from "@playwright/test";
import fs from "node:fs";

test("workspace renders, saves full schemes, and protects unsaved edits", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "让画面，清晰一点。" }),
  ).toBeVisible();
  await page
    .getByRole("spinbutton", { name: "亮度数值", exact: true })
    .fill("20");
  await page
    .getByRole("spinbutton", { name: "Gamma数值", exact: true })
    .fill("1.2");
  await page.getByRole("button", { name: "保存方案", exact: true }).click();
  await page
    .getByRole("button", { name: "室内 · 暗部可见", exact: true })
    .click();
  await expect(
    page.getByRole("spinbutton", { name: "亮度数值", exact: true }),
  ).toHaveValue("0");
  await page
    .getByRole("button", { name: "均衡 · 三角洲", exact: true })
    .click();
  await expect(
    page.getByRole("spinbutton", { name: "亮度数值", exact: true }),
  ).toHaveValue("20");
  await expect(
    page.getByRole("spinbutton", { name: "Gamma数值", exact: true }),
  ).toHaveValue("1.20");
  await page
    .getByRole("spinbutton", { name: "亮度数值", exact: true })
    .fill("25");
  await page
    .getByRole("button", { name: "室外 · 保留高光", exact: true })
    .click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.getByRole("button", { name: "取消", exact: true }).click();
  await expect(
    page.getByRole("spinbutton", { name: "亮度数值", exact: true }),
  ).toHaveValue("25");
  await page
    .getByRole("button", { name: "室外 · 保留高光", exact: true })
    .click();
  await page.getByRole("button", { name: "保存并切换", exact: true }).click();
  await page
    .getByRole("button", { name: "均衡 · 三角洲", exact: true })
    .click();
  await expect(
    page.getByRole("spinbutton", { name: "亮度数值", exact: true }),
  ).toHaveValue("25");
  expect(errors).toEqual([]);
});
test("local reference image renders with LUT and keyboard split control", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.locator("#app")).toBeVisible();
  const before = await page
    .locator("#adjusted")
    .evaluate((c) =>
      (c as HTMLCanvasElement)
        .getContext("2d")
        ?.getImageData(200, 300, 1, 1)
        .data.join(","),
    );
  await page
    .getByRole("spinbutton", { name: "Gamma数值", exact: true })
    .fill("1.4");
  await page.keyboard.press("Tab");
  await expect
    .poll(() =>
      page
        .locator("#adjusted")
        .evaluate((c) =>
          (c as HTMLCanvasElement)
            .getContext("2d")
            ?.getImageData(200, 300, 1, 1)
            .data.join(","),
        ),
    )
    .not.toBe(before);
  const png = await page
    .locator("#original")
    .evaluate((c) => (c as HTMLCanvasElement).toDataURL().split(",")[1]);
  await page.locator("#image-file").setInputFiles({
    name: "reference.png",
    mimeType: "image/png",
    buffer: Buffer.from(png, "base64"),
  });
  await expect(page.locator("#image-name")).toHaveText("reference.png");
  await page.getByRole("slider", { name: "原始与调整后的分隔位置" }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(page.locator("#split")).toHaveValue("51");
  await expect(page.locator("#adjusted")).toHaveCSS(
    "clip-path",
    "inset(0px 0px 0px 51%)",
  );
});
test("settings validates game process names and exposes startup options", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "应用设置", exact: true }).click();
  await expect(
    page.getByRole("switch", { name: "随 Windows 登录启动" }),
  ).toBeVisible();
  await page.locator("#process-names").fill("C:\\game.exe");
  await page.getByRole("button", { name: "保存设置", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("不要填写路径");
  await page.locator("#process-names").fill("game.exe");
  await page.locator("#poll-seconds").fill("5");
  await page.getByRole("button", { name: "保存设置", exact: true }).click();
  await expect(page.locator("#settings-message")).toHaveText("设置已保存");
  await page.getByRole("button", { name: "跟随游戏", exact: true }).click();
  await expect(page.locator("#game-hint")).toContainText("5 秒");
});
test("desktop and compact screenshots have no horizontal overflow", async ({
  page,
}) => {
  fs.mkdirSync("artifacts/ui", { recursive: true });
  await page.goto("/");
  await expect(page.locator("#app")).toBeVisible();
  await page.waitForFunction(
    () =>
      document.querySelector<HTMLCanvasElement>("#adjusted")?.width === 1280,
  );
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.screenshot({
    path: "artifacts/ui/workspace.png",
    fullPage: true,
    animations: "disabled",
  });
  await page.getByRole("button", { name: "应用设置", exact: true }).click();
  await page.screenshot({
    path: "artifacts/ui/settings.png",
    fullPage: true,
    animations: "disabled",
  });
  await page.getByRole("button", { name: "调色工作台", exact: true }).click();
  await page.setViewportSize({ width: 980, height: 740 });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.screenshot({
    path: "artifacts/ui/compact.png",
    fullPage: true,
    animations: "disabled",
  });
});

test("sidebar renames and binds schemes without saving color drafts", async ({
  page,
}) => {
  await page.goto("/");
  await page
    .getByRole("spinbutton", { name: "亮度数值", exact: true })
    .fill("25");
  await page
    .getByRole("button", { name: "管理方案：均衡 · 三角洲", exact: true })
    .click();
  await page.getByLabel("方案名称", { exact: true }).last().fill("Game A");
  await page.getByLabel("绑定进程文件名").fill("C:\\game.exe");
  await page.getByRole("button", { name: "保存更改", exact: true }).click();
  await expect(page.locator("#profile-message")).toContainText("不要填写路径");
  await page.getByLabel("绑定进程文件名").fill("game.exe\nGAME.EXE");
  await page.getByRole("button", { name: "保存更改", exact: true }).click();
  await expect(page.locator("#profile-message")).toContainText("重复");
  await page.getByLabel("绑定进程文件名").fill("game-a.exe");
  await page.locator("#pick-profile-process").click();
  await page
    .getByRole("button", { name: "DeltaForceClient.exe", exact: true })
    .click();
  await expect(page.getByLabel("绑定进程文件名")).toHaveValue(
    "game-a.exe\nDeltaForceClient.exe",
  );
  await page.getByRole("button", { name: "保存更改", exact: true }).click();
  await expect(page.locator("#profile-dialog")).not.toBeVisible();
  await expect(
    page.getByRole("button", { name: "Game A", exact: true }),
  ).toContainText("game-a.exe");
  await expect(page.locator("#profile-name")).toHaveValue("Game A");
  await expect(
    page.getByRole("spinbutton", { name: "亮度数值", exact: true }),
  ).toHaveValue("25");
  await expect(page.locator("#draft-state")).toHaveText("有未保存的调整");
  await page.getByRole("button", { name: "保存方案", exact: true }).click();
  await expect(page.locator("#draft-state")).toHaveText("当前方案已保存");
  await page.getByRole("button", { name: "应用设置", exact: true }).click();
  await page.locator("#process-names").fill("");
  await page.getByRole("button", { name: "保存设置", exact: true }).click();
  await expect(page.locator("#settings-message")).toHaveText("设置已保存");
  await page.getByRole("button", { name: "跟随游戏", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "跟随游戏", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await page
    .getByRole("button", { name: "管理方案：Game A", exact: true })
    .click();
  await expect(page.getByLabel("绑定进程文件名")).toHaveValue(
    "game-a.exe\nDeltaForceClient.exe",
  );
  await page.getByLabel("绑定进程文件名").fill("");
  await page.getByRole("button", { name: "保存更改", exact: true }).click();
  await page.getByRole("button", { name: "调色工作台", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Game A", exact: true }),
  ).not.toContainText("game-a.exe");
  await expect(page.locator("#draft-state")).toHaveText("当前方案已保存");
});

test("pointer and keyboard sorting preserve selection and unsaved adjustments", async ({
  page,
}) => {
  await page.goto("/");
  await page
    .getByRole("spinbutton", { name: "亮度数值", exact: true })
    .fill("25");
  const handle = await page
    .getByRole("button", { name: "排序方案：均衡 · 三角洲", exact: true })
    .boundingBox();
  const target = await page.locator(".profile-row").nth(2).boundingBox();
  expect(handle).not.toBeNull();
  expect(target).not.toBeNull();
  if (!handle || !target) throw new Error("Missing reorder controls");
  await page.mouse.move(
    handle.x + handle.width / 2,
    handle.y + handle.height / 2,
  );
  await page.mouse.down();
  await page.mouse.move(
    handle.x + handle.width / 2,
    target.y + target.height - 8,
    { steps: 12 },
  );
  await page.mouse.up();
  await expect(page.locator(".profile-card strong")).toHaveText([
    "室内 · 暗部可见",
    "室外 · 保留高光",
    "均衡 · 三角洲",
    "中性 · 校准基准",
  ]);
  await expect(
    page.getByRole("button", { name: "均衡 · 三角洲", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await expect(
    page.getByRole("spinbutton", { name: "亮度数值", exact: true }),
  ).toHaveValue("25");
  await page
    .getByRole("button", { name: "排序方案：均衡 · 三角洲", exact: true })
    .focus();
  await page.keyboard.press("ArrowUp");
  await expect(page.locator(".profile-card strong")).toHaveText([
    "室内 · 暗部可见",
    "均衡 · 三角洲",
    "室外 · 保留高光",
    "中性 · 校准基准",
  ]);
  await expect(
    page.getByRole("button", { name: "排序方案：均衡 · 三角洲", exact: true }),
  ).toBeFocused();
  const currentHandle = await page
    .getByRole("button", { name: "排序方案：均衡 · 三角洲", exact: true })
    .boundingBox();
  const firstRow = await page.locator(".profile-row").first().boundingBox();
  if (!currentHandle || !firstRow) throw new Error("Missing reorder controls");
  const originX = currentHandle.x + currentHandle.width / 2;
  const originY = currentHandle.y + currentHandle.height / 2;
  await page.mouse.move(originX, originY);
  await page.mouse.down();
  await page.mouse.move(originX, firstRow.y + 8, { steps: 6 });
  await page.mouse.move(originX, originY, { steps: 6 });
  await page.mouse.up();
  await expect(page.locator(".profile-card strong")).toHaveText([
    "室内 · 暗部可见",
    "均衡 · 三角洲",
    "室外 · 保留高光",
    "中性 · 校准基准",
  ]);
  await page
    .getByRole("button", { name: "室内 · 暗部可见", exact: true })
    .click();
  await expect(page.locator("#confirm-dialog")).toBeVisible();
  await page.getByRole("button", { name: "取消", exact: true }).click();
  await expect(
    page.getByRole("spinbutton", { name: "亮度数值", exact: true }),
  ).toHaveValue("25");
});

test("sidebar deletion confirms, clears default references and protects the last scheme", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "应用设置", exact: true }).click();
  await page
    .locator("#game-profile")
    .selectOption({ label: "室内 · 暗部可见" });
  await page.getByRole("button", { name: "保存设置", exact: true }).click();
  await page.getByRole("button", { name: "调色工作台", exact: true }).click();
  await page
    .getByRole("spinbutton", { name: "亮度数值", exact: true })
    .fill("25");
  await page
    .getByRole("button", { name: "管理方案：室内 · 暗部可见", exact: true })
    .click();
  await page.getByRole("button", { name: "删除方案", exact: true }).click();
  await page
    .locator("#confirm-dialog")
    .getByRole("button", { name: "取消", exact: true })
    .click();
  await expect(page.locator("#profile-dialog")).toBeVisible();
  await page.getByRole("button", { name: "删除方案", exact: true }).click();
  await page
    .locator("#confirm-dialog")
    .getByRole("button", { name: "删除", exact: true })
    .click();
  await expect(page.locator(".profile-card")).toHaveCount(3);
  await expect(
    page.getByRole("spinbutton", { name: "亮度数值", exact: true }),
  ).toHaveValue("25");
  await page.getByRole("button", { name: "应用设置", exact: true }).click();
  await expect(page.locator("#game-profile option")).toHaveCount(3);
  await expect(page.locator("#game-profile option:checked")).toHaveText(
    "均衡 · 三角洲",
  );
  for (const name of ["室外 · 保留高光", "中性 · 校准基准"]) {
    await page
      .getByRole("button", { name: `管理方案：${name}`, exact: true })
      .click();
    await page.getByRole("button", { name: "删除方案", exact: true }).click();
    await page
      .locator("#confirm-dialog")
      .getByRole("button", { name: "删除", exact: true })
      .click();
  }
  await expect(page.locator(".profile-card")).toHaveCount(1);
  await page
    .getByRole("button", { name: "管理方案：均衡 · 三角洲", exact: true })
    .click();
  await page.getByRole("button", { name: "删除方案", exact: true }).click();
  await expect(page.locator("#profile-message")).toHaveText("至少保留一个方案");
  await expect(page.locator(".profile-card")).toHaveCount(1);
});

for (const viewport of [
  { width: 1440, height: 1000 },
  { width: 980, height: 740 },
  { width: 860, height: 620 },
]) {
  test(`expanded ICC stays above the apply button at ${viewport.width}x${viewport.height}`, async ({
    page,
  }) => {
    await page.setViewportSize(viewport);
    await page.goto("/");
    await expect(page.locator("#app")).toBeVisible();
    await expect(page.locator(".profile-card").first()).toBeInViewport();
    const studio = await page.locator(".studio").boundingBox();
    const footer = await page.locator(".footer").boundingBox();
    await page.locator("summary").filter({ hasText: "RGB 增益" }).click();
    await page.locator("#icc").evaluate((select) => {
      const option = document.createElement("option");
      option.textContent = `${"Very-long-ICC-profile-name-".repeat(10)}.icc`;
      (select as HTMLSelectElement).add(option);
      (select as HTMLSelectElement).selectedIndex = 1;
    });
    await page.locator("summary").filter({ hasText: "ICC 配置" }).click();
    await page.locator(".inspector-scroll").evaluate((pane) => {
      pane.scrollTop = pane.scrollHeight;
    });
    await expect(page.locator("#icc")).toBeInViewport();
    await expect(page.locator("#apply")).toBeInViewport();
    expect(await page.locator(".studio").boundingBox()).toEqual(studio);
    expect(await page.locator(".footer").boundingBox()).toEqual(footer);
    const layout = await page.evaluate(() => {
      const icc = document.querySelector("#icc")!.getBoundingClientRect();
      const actions = document
        .querySelector(".icc-actions")!
        .getBoundingClientRect();
      const apply = document
        .querySelector(".apply-area")!
        .getBoundingClientRect();
      const pane = document.querySelector(".inspector-scroll")!;
      return {
        controlsAboveApply:
          icc.bottom <= apply.top && actions.bottom <= apply.top,
        noPaneOverflow: pane.scrollWidth <= pane.clientWidth,
        noPageOverflow: document.documentElement.scrollWidth <= innerWidth,
      };
    });
    expect(layout).toEqual({
      controlsAboveApply: true,
      noPaneOverflow: true,
      noPageOverflow: true,
    });
    await page.getByRole("button", { name: "导入 ICC", exact: true }).click();
    await expect(page.locator("#toast")).toContainText("Windows 客户端");
    fs.mkdirSync("artifacts/ui", { recursive: true });
    await page.screenshot({
      path: `artifacts/ui/icc-${viewport.width}.png`,
      animations: "disabled",
    });
  });
}
