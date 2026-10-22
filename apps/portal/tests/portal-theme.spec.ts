import { test, expect, type Page } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { sectionDefinitions } from "../app/navigation";

const auditEvents = [
  {
    id: 1,
    action: "login",
    actor: "admin",
    entity_id: "administrator-1",
    details: {},
    created_at: "2026-10-10T09:20:00Z",
  },
  {
    id: 2,
    action: "training_succeeded",
    actor: "admin",
    entity_id: "model-1",
    details: { rows: 2000 },
    created_at: "2026-10-10T09:18:00Z",
  },
  {
    id: 3,
    action: "training_failed",
    actor: "admin",
    entity_id: "job-1",
    details: { reason: "Dataset needs more representative records." },
    created_at: "2026-10-10T09:15:00Z",
  },
  {
    id: 4,
    action: "auth-page",
    actor: "service-operator-with-a-long-identifier@example.test",
    entity_id: "resource-" + "a".repeat(80),
    details: { information: "Long audit detail ".repeat(50) },
    created_at: "2026-10-10T09:12:00Z",
  },
  {
    id: 5,
    action: "endpoint_created",
    actor: "admin",
    entity_id: "endpoint-1",
    details: {},
    created_at: "2026-10-10T09:10:00Z",
  },
];
async function mockWorkspace(page: Page, initiallyAuthenticated = true) {
  let authenticated = initiallyAuthenticated;
  await page.route("**/api/**", async (route) => {
    const path = new URL(route.request().url()).pathname.slice(5);
    if (path === "login") {
      authenticated = true;
      await route.fulfill({ json: { ok: true } });
      return;
    }
    if (path === "logout") {
      authenticated = false;
      await route.fulfill({ json: { ok: true } });
      return;
    }
    if (!authenticated) {
      await route.fulfill({ status: 401, json: { error: "Session expired" } });
      return;
    }
    const data: Record<string, unknown> = {
      overview: { endpoints: 3, pipelines: 2, models: 4, releases: 2 },
      apis: [{ id: "api-1", name: "Orders API", host: "api.example.test" }],
      endpoints: [
        {
          id: "endpoint-1",
          api_id: "api-1",
          name: "Create order",
          method: "POST",
          path_template: "/orders",
        },
      ],
      "audit-events": auditEvents,
      "onboarding/provider": null,
      servers: [
        {
          id: "server-1",
          name: "Production proxy",
          address: "proxy.internal",
          port: 22,
          username: "pragma-deploy",
          discovery: null,
        },
      ],
    };
    await route.fulfill({ json: data[path] ?? [] });
  });
}
async function ready(page: Page, label: string) {
  await expect(
    page.getByRole("heading", { name: label, exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("Connecting to your management API…"),
  ).toBeHidden();
}

test("all sections support direct links and refresh without reverting to Overview", async ({
  page,
}) => {
  await mockWorkspace(page);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  for (const section of sectionDefinitions) {
    await page.goto(`/?section=${section.id}`);
    await ready(page, section.label);
    await page.reload();
    await ready(page, section.label);
    await expect(
      page.getByRole("button", { name: section.label, exact: true }),
    ).toHaveAttribute("aria-current", "page");
  }
  expect(errors).toEqual([]);
});

test("navigation shortcuts, browser history, and collapse state remain consistent", async ({
  page,
}) => {
  await mockWorkspace(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Configure an endpoint" }).click();
  await expect(page).toHaveURL(/section=endpoints/);
  await page.getByRole("button", { name: "Datasets", exact: true }).click();
  await page.getByLabel("Dataset name").fill("Draft capture");
  await page.getByRole("button", { name: "Audit log", exact: true }).click();
  await expect(page).toHaveURL(/section=audit-log/);
  await page.goBack();
  await ready(page, "Datasets");
  await expect(page.getByLabel("Dataset name")).toHaveValue("Draft capture");
  await page.goForward();
  await ready(page, "Audit log");
  await page.getByRole("button", { name: "Create order", exact: true }).click();
  await expect(page).toHaveURL(/section=endpoints/);
  await page
    .getByRole("button", { name: "Collapse sidebar", exact: true })
    .click();
  await page.reload();
  await ready(page, "Endpoints");
  await expect(page.locator(".sidebar")).toBeHidden();
  await expect(page.locator(".main")).toHaveCSS("margin-left", "0px");
  await page
    .getByRole("button", { name: "Expand sidebar", exact: true })
    .click();
  await expect(page.locator(".sidebar")).toBeVisible();
  await page.goto("/?section=not-a-section&source=bookmark");
  await ready(page, "Overview");
  await expect(page).toHaveURL(/\?source=bookmark$/);
});

test("expired sessions return to the selected section and explicit sign-out clears it", async ({
  page,
}) => {
  await mockWorkspace(page, false);
  await page.goto("/?section=servers");
  await expect(page).toHaveURL(/\/login\?section=servers$/);
  await page.getByLabel("Password", { exact: true }).fill("test-only");
  await page.getByRole("button", { name: "Sign in →" }).click();
  await ready(page, "Servers");
  await expect(page).toHaveURL(/\/\?section=servers$/);
  await page.getByRole("button", { name: "Sign out ↗", exact: true }).click();
  await expect(page).toHaveURL(/\/login$/);
  await page.goto("/login?section=https://external.invalid");
  await page.getByLabel("Password", { exact: true }).fill("test-only");
  await page.getByRole("button", { name: "Sign in →" }).click();
  await ready(page, "Overview");
  await expect(page).toHaveURL(/\/$/);
});

test("audit badges stay compact, semantic, and independent of action text", async ({
  page,
}) => {
  await mockWorkspace(page);
  await page.goto("/?section=audit-log");
  await ready(page, "Audit log");
  const login = page
    .locator(".audit-table .badge")
    .filter({ hasText: /^login$/ });
  await expect(login).toHaveClass("badge badge--neutral");
  expect((await login.boundingBox())!.height).toBeLessThan(40);
  expect(
    (await login.locator("xpath=ancestor::tr").boundingBox())!.height,
  ).toBeLessThan(120);
  await expect(page.locator(".badge--success")).toHaveText("succeeded");
  await expect(page.locator(".badge--danger")).toHaveText("failed");
  await expect(page.locator(".audit-table .auth-page")).toHaveCount(0);
  await page.getByLabel("Search activity").fill("auth-page");
  await expect(page.locator(".audit-table tbody tr")).toHaveCount(1);
  await page.getByText("View details", { exact: true }).click();
  await expect(page.locator(".audit-table pre")).toBeVisible();
  await page.setViewportSize({ width: 390, height: 844 });
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(390);
  await page.getByLabel("Search activity").fill("");
  await page
    .getByRole("combobox", { name: "Action", exact: true })
    .selectOption("login");
  await expect(page.locator(".audit-table tbody tr")).toHaveCount(1);
});

test("site theme, responsive navigation, contrast, and visual review", async ({
  page,
}) => {
  await mockWorkspace(page);
  const screenshots = "../../build/screenshots/portal-theme";
  mkdirSync(screenshots, { recursive: true });
  for (const size of [
    { name: "desktop", width: 1440, height: 1000 },
    { name: "mobile", width: 390, height: 844 },
  ]) {
    await page.setViewportSize(size);
    for (const section of [
      "login",
      "overview",
      "feature-pipelines",
      "datasets",
      "audit-log",
      "servers",
    ]) {
      await page.goto(section === "login" ? "/login" : `/?section=${section}`);
      if (section !== "login")
        await ready(
          page,
          sectionDefinitions.find((s) => s.id === section)!.label,
        );
      await page.evaluate(() => document.fonts.ready);
      await expect(page.locator("body")).toHaveCSS(
        "background-color",
        "rgb(9, 17, 19)",
      );
      expect(
        await page.evaluate(() => document.documentElement.scrollWidth),
      ).toBeLessThanOrEqual(size.width);
      await page.screenshot({
        path: `${screenshots}/${size.name}-${section}.png`,
        fullPage: false,
      });
    }
  }
  await page
    .getByRole("button", { name: "Open navigation", exact: true })
    .click();
  await expect(page.locator(".sidebar")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("button", { name: "Open navigation", exact: true }),
  ).toBeFocused();
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page
    .getByRole("button", { name: "Open navigation", exact: true })
    .click();
  await expect(page.locator(".sidebar")).toHaveCSS("transition-duration", "0s");
  await page.getByRole("button", { name: "Datasets", exact: true }).click();
  await ready(page, "Datasets");
  await expect(page.locator(".sidebar")).toBeHidden();
  await page.reload();
  await ready(page, "Datasets");
  const contrast = await page.evaluate(() => {
    const css = getComputedStyle(document.documentElement);
    const luminance = (hex: string) => {
      const values = hex
        .trim()
        .slice(1)
        .match(/../g)!
        .map((v) => parseInt(v, 16) / 255)
        .map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
      return values[0] * 0.2126 + values[1] * 0.7152 + values[2] * 0.0722;
    };
    return [
      ["--ink", "--panel"],
      ["--muted", "--panel-raised"],
      ["--subtle", "--input"],
      ["--accent", "--accent-ink"],
      ["--success", "--success-bg"],
      ["--warning", "--warning-bg"],
      ["--danger", "--danger-bg"],
      ["--information", "--information-bg"],
    ].map(([a, b]) => {
      const l1 = luminance(css.getPropertyValue(a)),
        l2 = luminance(css.getPropertyValue(b));
      return {
        pair: `${a}/${b}`,
        ratio: (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05),
      };
    });
  });
  for (const pair of contrast)
    expect(pair.ratio, pair.pair).toBeGreaterThanOrEqual(4.5);
});
