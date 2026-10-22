import { test, expect } from "@playwright/test";
test("navigation geometry and responsive controls", async ({ page }) => {
  await page.goto("/login");
  await page.getByLabel("Username").fill(process.env.ADMIN_USERNAME || "admin");
  await page
    .getByLabel("Password", { exact: true })
    .fill(process.env.ADMIN_PASSWORD!);
  await page.getByRole("button", { name: "Sign in →" }).click();
  await expect(
    page.getByRole("heading", { name: "Overview", exact: true }),
  ).toBeVisible();
  await expect(page.locator(".sidebar")).toHaveCSS("width", "238px");
  await page.screenshot({ path: "../../build/screenshots/desktop-layout.png" });
  await page
    .getByRole("button", { name: "Collapse sidebar", exact: true })
    .click();
  await expect(page.locator(".sidebar")).toBeHidden();
  await expect(page.locator(".main")).toHaveCSS("margin-left", "0px");
  await page.screenshot({
    path: "../../build/screenshots/collapsed-layout.png",
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  expect(
    await page
      .locator(".main")
      .evaluate((el) => getComputedStyle(el).marginLeft),
  ).toBe("0px");
  await expect(page.locator(".sidebar")).toBeHidden();
  await page
    .getByRole("button", { name: "Expand sidebar", exact: true })
    .click();
  await expect(page.locator(".sidebar")).toHaveCSS("width", "238px");
  await expect(page.locator(".sidebar")).toBeVisible();
  await expect(page.locator(".main")).toHaveCSS("margin-left", "238px");
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.locator(".sidebar")).toBeHidden();
  await page
    .getByRole("button", { name: "Open navigation", exact: true })
    .click();
  await expect(page.locator(".sidebar")).toHaveCSS(
    "transform",
    "matrix(1, 0, 0, 1, 0, 0)",
  );
  await page.screenshot({
    path: "../../build/screenshots/mobile-navigation.png",
  });
  await page.getByRole("button", { name: "Overview", exact: true }).click();
  await expect(page.locator(".sidebar")).toBeHidden();
  await page.screenshot({
    path: "../../build/screenshots/mobile-layout.png",
    fullPage: true,
  });
  await page
    .getByRole("button", { name: "Open navigation", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Feature pipelines", exact: true })
    .click();
  await expect(page.locator(".sidebar")).toBeHidden();
  await page
    .getByLabel("Extractor 1 name")
    .fill("a_very_long_extractor_name_for_layout_verification");
  await page.getByText("Runtime limits · advanced", { exact: true }).click();
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(390);
  await page.screenshot({
    path: "../../build/screenshots/mobile-pipeline.png",
    fullPage: true,
  });

  await page.emulateMedia({ reducedMotion: "reduce" });
  await page
    .getByRole("button", { name: "Open navigation", exact: true })
    .click();
  await expect(page.locator(".sidebar")).toHaveCSS("transition-duration", "0s");
});
