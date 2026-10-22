import { test, expect } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { execFileSync } from "node:child_process";

test("configure, import, train, and export through the portal", async ({
  page,
}) => {
  await page.goto("/login");
  await page.getByLabel("Username").fill(process.env.ADMIN_USERNAME || "admin");
  await page
    .getByLabel("Password", { exact: true })
    .fill(process.env.ADMIN_PASSWORD!);
  await page.getByRole("button", { name: "Sign in →" }).click();
  await expect(
    page.getByRole("heading", { name: "Overview", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("Connecting to your management API…"),
  ).not.toBeVisible();
  mkdirSync("../../build/screenshots", { recursive: true });
  await expect(page.locator(".sidebar")).toHaveCSS("width", "238px");
  await page.screenshot({
    path: "../../build/screenshots/overview.png",
    fullPage: true,
    animations: "disabled",
  });
  await page
    .getByRole("button", { name: "Collapse sidebar", exact: true })
    .click();
  await expect(page.locator(".sidebar")).toBeHidden();
  await expect(page.locator(".main")).toHaveCSS("margin-left", "0px");
  await page.reload();
  await expect(
    page.getByRole("button", { name: "Expand sidebar", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Expand sidebar", exact: true })
    .click();
  const suffix = Date.now().toString(36);
  await page
    .getByRole("button", { name: "Endpoints", exact: false })
    .first()
    .click();
  const apiForm = page.locator("form").filter({
    has: page.getByRole("button", { name: "Create API", exact: true }),
  });
  await apiForm.getByLabel("Name", { exact: true }).fill(`Browser ${suffix}`);
  await apiForm.getByLabel("API host").fill(`${suffix}.example.test`);
  await apiForm
    .getByRole("button", { name: "Create API", exact: true })
    .click();
  await expect(
    page.getByRole("status").filter({ hasText: "Saved successfully." }),
  ).toBeVisible();
  const endpointForm = page.locator("form").filter({
    has: page.getByRole("button", { name: "Create endpoint", exact: true }),
  });
  await endpointForm
    .locator("select[name=api_id]")
    .selectOption({ label: `Browser ${suffix} · ${suffix}.example.test` });
  await endpointForm
    .getByLabel("Name", { exact: true })
    .fill(`Orders ${suffix}`);
  await endpointForm.locator("select[name=method]").selectOption("POST");
  await endpointForm
    .getByLabel("Path template")
    .fill(`/orders-${suffix}/{order_id}`);
  await endpointForm
    .getByRole("button", { name: "Create endpoint", exact: true })
    .click();
  const row = page.getByRole("row").filter({ hasText: `Orders ${suffix}` });
  await row.getByRole("button", { name: "Configure →" }).click();
  await expect(
    page.getByRole("heading", { name: "Pipeline builder", exact: true }),
  ).toBeVisible();
  const controls = await page
    .locator(".builder-row")
    .nth(1)
    .locator("input,button")
    .evaluateAll((elements) =>
      elements.map((el) => {
        const r = el.getBoundingClientRect();
        return { top: r.top, height: r.height };
      }),
    );
  expect(new Set(controls.map((r) => r.height)).size).toBe(1);
  expect(new Set(controls.map((r) => r.top)).size).toBe(1);
  const requestJson = page.getByLabel("Request JSON");
  const readRequest = async () =>
    JSON.parse((await requestJson.textContent()) || "{}");
  expect((await readRequest()).headers).toEqual({ "X-Client-ID": "example" });
  expect((await readRequest()).path).toBe(`/orders-${suffix}/example`);
  await page.getByRole("button", { name: "+ Add field" }).click();
  const accountField = page.locator(".builder-row").nth(1);
  await accountField.getByLabel("Name").fill("account_id");
  await accountField.getByLabel("Source").selectOption("json_pointer");
  await accountField.getByLabel("JSON Pointer").fill("/account/id");
  expect((await readRequest()).body).toEqual({ account: { id: "example" } });
  await page.getByRole("button", { name: "+ Add field" }).click();
  const queryField = page.locator(".builder-row").nth(2);
  await queryField.getByLabel("Name").fill("region");
  await queryField.getByLabel("Source").selectOption("query_parameter");
  await queryField.getByLabel("Field").fill("region");
  expect((await readRequest()).query).toEqual({ region: "example" });
  await page.getByRole("button", { name: "+ Add field" }).click();
  const pathField = page.locator(".builder-row").nth(3);
  await pathField.getByLabel("Name").fill("order_id");
  await pathField.getByLabel("Source").selectOption("path_parameter");
  await pathField.getByLabel("Field").fill("order_id");
  expect((await readRequest()).path).toBe(`/orders-${suffix}/example`);
  await page.getByRole("tab", { name: "Pipeline JSON" }).click();
  const pipelineJson = JSON.parse(
    (await page.locator('pre[aria-label="Pipeline JSON"]').textContent()) ||
      "{}",
  );
  expect(pipelineJson.api_host).toBe(`${suffix}.example.test`);
  expect(pipelineJson.path_template).toBe(`/orders-${suffix}/{order_id}`);
  expect(pipelineJson.extractors).toHaveLength(4);
  await page.getByRole("tab", { name: "Request example" }).click();
  await page.getByText("Runtime limits · advanced", { exact: true }).click();
  await expect(page.getByLabel("Body inspection size (bytes)")).toHaveValue(
    "65536",
  );
  await page.screenshot({
    path: "../../build/screenshots/pipeline-controls.png",
    fullPage: true,
    animations: "disabled",
  });
  await page.getByRole("button", { name: "Validate & save version →" }).click();
  await expect(
    page
      .getByRole("status")
      .filter({ hasText: "New pipeline version created" }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Datasets", exact: false })
    .first()
    .click();
  await page.getByLabel("Dataset name").fill(`Browser baseline ${suffix}`);
  await page.getByRole("button", { name: /^Feature vectors/ }).click();
  await expect(page.getByText("Why the schema must match")).toBeVisible();
  const csv =
    "requests_per_minute,body_bytes\n" +
    Array.from(
      { length: 100 },
      (_, i) => `${5 + (i % 10)},${80 + (i % 35)}`,
    ).join("\n");
  await page.getByLabel("Feature vectors (CSV)").fill(csv);
  await page.getByRole("button", { name: "Preview & validate" }).click();
  await expect(
    page.getByRole("button", { name: "Accept import →" }),
  ).toBeVisible();
  await page.screenshot({
    path: "../../build/screenshots/datasets.png",
    fullPage: true,
    animations: "disabled",
  });
  await page.getByRole("button", { name: "Accept import →" }).click();
  const imported = page
    .locator(".list-row")
    .filter({ hasText: `Browser baseline ${suffix}` });
  await imported.getByRole("button", { name: "Train model →" }).click();
  await page.getByLabel("Trees", { exact: true }).fill("10");
  await page.getByRole("button", { name: "Start training →" }).click();
  const jobRow = page
    .getByRole("row")
    .filter({ hasText: `Browser baseline ${suffix}` });
  await expect(jobRow.getByText("succeeded", { exact: true })).toBeVisible({
    timeout: 60000,
  });
  const modelCard = page
    .locator(".model-card")
    .filter({ hasText: `Browser baseline ${suffix}` });
  await modelCard.getByRole("button", { name: "More", exact: true }).click();
  await expect(
    modelCard.getByRole("heading", { name: "Training provenance" }),
  ).toBeVisible();
  await expect(
    modelCard.locator("dd").filter({ hasText: `Browser baseline ${suffix}` }),
  ).toBeVisible();
  await modelCard
    .getByRole("button", { name: "Test this model", exact: true })
    .click();
  await page.getByLabel("Vector (JSON array)").fill("[10,100]");
  await page.getByLabel("Check threshold", { exact: true }).fill("0.6");
  await page.getByLabel("Block threshold", { exact: true }).fill("0.8");
  await page.getByRole("button", { name: "Score vector", exact: true }).click();
  await expect(page.locator(".result")).toBeVisible();
  await page.screenshot({
    path: "../../build/screenshots/model-details.png",
    fullPage: true,
    animations: "disabled",
  });
  await page
    .getByRole("button", { name: "Request replay", exact: true })
    .click();
  const selectedPipeline = await page
    .getByLabel("Pipeline version")
    .inputValue();
  const alternatives = await page
    .getByLabel("Pipeline version")
    .locator("option")
    .evaluateAll((options) =>
      options.map((o) => (o as HTMLOptionElement).value).filter(Boolean),
    );
  const otherPipeline = alternatives.find((id) => id !== selectedPipeline);
  if (otherPipeline) {
    await page.getByLabel("Pipeline version").selectOption(otherPipeline);
    await expect(page.getByLabel("Optional model")).toHaveValue("");
  }
  await page
    .getByRole("button", { name: "Training & models", exact: true })
    .click();
  await modelCard
    .getByRole("button", { name: "Create release →", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Releases", exact: false })
    .first()
    .click();
  await page.getByLabel("Release name").fill(`Browser release ${suffix}`);
  const modelChoice = page
    .locator(".release-choice")
    .filter({ hasText: `Browser baseline ${suffix}` });
  await expect(modelChoice.getByRole("checkbox")).toBeChecked();
  await expect(modelChoice.getByLabel("Check threshold")).toHaveValue("0.6");
  await modelChoice.getByLabel("Check threshold").fill("0.9");
  await expect(
    page.getByRole("button", { name: "Validate & create release →" }),
  ).toBeDisabled();
  await expect(
    modelChoice.getByText("Use 0 ≤ check < block ≤ 1."),
  ).toBeVisible();
  await modelChoice.getByLabel("Check threshold").fill("0.6");
  await modelChoice.getByLabel("Block threshold").fill("0.8");
  await page
    .getByRole("button", { name: "Validate & create release →" })
    .click();
  const releaseRow = page
    .locator(".list-row")
    .filter({ hasText: `Browser release ${suffix}` });
  await expect(releaseRow).toBeVisible();
  await expect(
    releaseRow.getByRole("link", { name: "Download bundle", exact: false }),
  ).toHaveCSS("cursor", "pointer");
  const downloadPromise = page.waitForEvent("download");
  await releaseRow
    .getByRole("link", { name: "Download bundle", exact: false })
    .click();
  const download = await downloadPromise;
  expect(download.suggestedFilename()).toMatch(/pragmachange-.*\.tar\.gz/);
  await download.saveAs("../../build/e2e/browser-release.tar.gz");
  await page.screenshot({
    path: "../../build/screenshots/releases.png",
    fullPage: true,
    animations: "disabled",
  });
  const bundle = page
    .locator(".release-bundle")
    .filter({ hasText: `Browser release ${suffix}` });
  await expect(bundle.locator("summary")).toHaveCSS("cursor", "pointer");
  await expect(bundle).toHaveCSS("cursor", "auto");
  await bundle.getByText("How to deploy", { exact: true }).click();
  await expect(
    bundle.getByText("Validate, install, and reload", { exact: true }),
  ).toBeVisible();
  await page.getByLabel("Deployment proxy").selectOption("apache");
  await bundle.getByText("How to deploy on Apache", { exact: true }).click();
  await expect(
    bundle.getByText("Install the matching package once", { exact: true }),
  ).toBeVisible();
  const apacheDownloadPromise = page.waitForEvent("download");
  await releaseRow
    .getByRole("link", { name: "Download bundle", exact: false })
    .click();
  const apacheDownload = await apacheDownloadPromise;
  expect(apacheDownload.suggestedFilename()).toMatch(/-apache\.tar\.gz$/);
  const apachePath = "../../build/e2e/browser-apache-release.tar.gz";
  await apacheDownload.saveAs(apachePath);
  expect(
    execFileSync("tar", ["-tzf", apachePath], { encoding: "utf8" }),
  ).toContain("apache.conf.example");
  expect(
    execFileSync("tar", ["-xOf", apachePath, "release.json"], {
      encoding: "utf8",
    }),
  ).toBe(
    execFileSync(
      "tar",
      ["-xOf", "../../build/e2e/browser-release.tar.gz", "release.json"],
      { encoding: "utf8" },
    ),
  );
  const downloadUrl = await releaseRow
    .getByRole("link", { name: "Download bundle", exact: false })
    .getAttribute("href");
  expect(
    (
      await page.request.get(
        downloadUrl!.replace("target=apache", "target=envoy"),
      )
    ).status(),
  ).toBe(400);
  await page.setViewportSize({ width: 390, height: 844 });
  await expect
    .poll(() => page.evaluate(() => document.documentElement.scrollWidth))
    .toBeLessThanOrEqual(390);
  await page.screenshot({
    path: "../../build/screenshots/apache-deployment-mobile.png",
    fullPage: true,
    animations: "disabled",
  });
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.getByRole("button", { name: "Audit log", exact: true }).click();
  await page.getByLabel("Search activity").fill(`Browser release ${suffix}`);
  await expect(page.locator("tbody tr")).toHaveCount(1);
  await page.getByLabel("Search activity").fill("no-such-event-unique");
  await expect(
    page.getByText("No activity matches these filters."),
  ).toBeVisible();
  await page.getByLabel("Search activity").fill("");
  await page.screenshot({
    path: "../../build/screenshots/audit.png",
    fullPage: true,
    animations: "disabled",
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await page
    .getByRole("button", { name: "Open navigation", exact: true })
    .click();
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("button", { name: "Open navigation", exact: true }),
  ).toBeFocused();
  await page
    .getByRole("button", { name: "Open navigation", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Overview", exact: false })
    .first()
    .click();
  await page.screenshot({
    path: "../../build/screenshots/mobile.png",
    fullPage: true,
    animations: "disabled",
  });
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(390);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.getByRole("button", { name: "Sign out", exact: false }).focus();
  await page.keyboard.press("Enter");
  await expect(
    page.getByRole("heading", { name: "Welcome back" }),
  ).toBeVisible();
});
