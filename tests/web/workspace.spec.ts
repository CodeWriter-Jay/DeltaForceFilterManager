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
  await page.getByRole("button", { name: /室内 · 暗部可见/ }).click();
  await expect(
    page.getByRole("spinbutton", { name: "亮度数值", exact: true }),
  ).toHaveValue("0");
  await page.getByRole("button", { name: /均衡 · 三角洲/ }).click();
  await expect(
    page.getByRole("spinbutton", { name: "亮度数值", exact: true }),
  ).toHaveValue("20");
  await expect(
    page.getByRole("spinbutton", { name: "Gamma数值", exact: true }),
  ).toHaveValue("1.20");
  await page
    .getByRole("spinbutton", { name: "亮度数值", exact: true })
    .fill("25");
  await page.getByRole("button", { name: /室外 · 保留高光/ }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.getByRole("button", { name: "取消", exact: true }).click();
  await expect(
    page.getByRole("spinbutton", { name: "亮度数值", exact: true }),
  ).toHaveValue("25");
  await page.getByRole("button", { name: /室外 · 保留高光/ }).click();
  await page.getByRole("button", { name: "保存并切换", exact: true }).click();
  await page.getByRole("button", { name: /均衡 · 三角洲/ }).click();
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
